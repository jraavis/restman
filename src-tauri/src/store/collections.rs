//! Collection (and folder) repository. A folder is a collection with a
//! `parent_id`; the frontend assembles the tree from the flat list `list`
//! returns, ordered by `(parent_id, sort_order)`.

use crate::error::{AppError, AppResult};
use crate::model::auth::AuthConfig;
use crate::model::Collection;
use crate::util::now_millis;
use rusqlite::{params, params_from_iter, Connection};
use uuid::Uuid;

const SELECT: &str =
    "SELECT id, workspace_id, parent_id, name, description, sort_order, created_at, updated_at, auth_json FROM collections";

fn row_to_collection(r: &rusqlite::Row) -> rusqlite::Result<Collection> {
    let auth_json: String = r.get(8)?;
    Ok(Collection {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        parent_id: r.get(2)?,
        name: r.get(3)?,
        description: r.get(4)?,
        auth: serde_json::from_str(&auth_json).unwrap_or_default(),
        sort_order: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> AppResult<Vec<Collection>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE workspace_id = ?1 ORDER BY parent_id IS NOT NULL, sort_order ASC, created_at ASC"
    ))?;
    let rows = stmt.query_map(params![workspace_id], row_to_collection)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Direct children of `parent_id` (`None` = top-level) within `workspace_id`.
/// Used by `interop::apply_import`/`collect` to walk the tree one level at
/// a time without pulling the whole workspace.
pub fn list_children(conn: &Connection, workspace_id: &str, parent_id: Option<&str>) -> AppResult<Vec<Collection>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE workspace_id = ?1 AND parent_id IS ?2 ORDER BY sort_order ASC, created_at ASC"
    ))?;
    let rows = stmt.query_map(params![workspace_id, parent_id], row_to_collection)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Collection> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_collection)
        .map_err(|_| AppError::NotFound(format!("collection {id}")))
}

pub fn create(
    conn: &Connection,
    workspace_id: &str,
    parent_id: Option<&str>,
    name: &str,
    description: Option<&str>,
) -> AppResult<Collection> {
    let now = now_millis();
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM collections WHERE workspace_id = ?1 AND parent_id IS ?2",
        params![workspace_id, parent_id],
        |r| r.get(0),
    )?;
    let c = Collection {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        parent_id: parent_id.map(str::to_string),
        name: name.to_string(),
        description: description.map(str::to_string),
        auth: AuthConfig::default(),
        sort_order: next_order,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO collections (id, workspace_id, parent_id, name, description, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![c.id, c.workspace_id, c.parent_id, c.name, c.description, c.sort_order, c.created_at, c.updated_at],
    )?;
    Ok(c)
}

pub fn update(conn: &Connection, id: &str, name: &str, description: Option<&str>) -> AppResult<Collection> {
    let n = conn.execute(
        "UPDATE collections SET name = ?2, description = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, description, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    get(conn, id)
}

/// Persists a collection's default auth. Separate from `update` (name/desc)
/// so editing auth doesn't require threading it through every other caller.
/// Secrets are written to the keychain (masked) by `crate::auth::persist`
/// before the JSON is stored, so `auth_json` never holds a real secret.
pub fn update_auth(conn: &Connection, id: &str, auth: AuthConfig) -> AppResult<Collection> {
    let owner = crate::auth::owner_key("collection", id);
    let masked = crate::auth::persist(&owner, auth)?;
    let n = conn.execute(
        "UPDATE collections SET auth_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, serde_json::to_string(&masked)?, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    get(conn, id)
}

/// Deletes `id` and, since collections have no `ON DELETE CASCADE` in the
/// schema, also explicitly deletes every nested sub-collection and every
/// request inside any of them — otherwise those rows would silently survive
/// as unreachable orphans (no FK to reject the delete, no parent left to
/// find them through).
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let ids = self_and_descendants(conn, id)?;
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    // Collected before the DELETE below — these rows (and the secrets their
    // owner keys point at) are about to disappear with no FK cascade to the
    // keychain, so each owner needs an explicit sweep.
    let mut req_id_stmt = conn.prepare(&format!("SELECT id FROM requests WHERE collection_id IN ({placeholders})"))?;
    let request_ids: Vec<String> = req_id_stmt
        .query_map(params_from_iter(&ids), |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    conn.execute(
        &format!("DELETE FROM requests WHERE collection_id IN ({placeholders})"),
        params_from_iter(&ids),
    )?;
    let n = conn.execute(
        &format!("DELETE FROM collections WHERE id IN ({placeholders})"),
        params_from_iter(&ids),
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    for rid in &request_ids {
        crate::auth::sweep(&crate::auth::owner_key("request", rid));
    }
    for cid in &ids {
        crate::auth::sweep(&crate::auth::owner_key("collection", cid));
    }
    Ok(())
}

fn self_and_descendants(conn: &Connection, id: &str) -> AppResult<Vec<String>> {
    let mut ids = vec![id.to_string()];
    let mut frontier = vec![id.to_string()];
    while let Some(parent) = frontier.pop() {
        let mut stmt = conn.prepare("SELECT id FROM collections WHERE parent_id = ?1")?;
        let children: Vec<String> = stmt
            .query_map(params![parent], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        frontier.extend(children.iter().cloned());
        ids.extend(children);
    }
    Ok(ids)
}

/// Reparent `id` under `new_parent_id` (`None` = move to top level).
/// Rejects moving a collection into itself or one of its own descendants.
pub fn move_to(conn: &Connection, id: &str, new_parent_id: Option<&str>) -> AppResult<Collection> {
    if let Some(target) = new_parent_id {
        if target == id {
            return Err(AppError::Other("cannot move a collection into itself".into()));
        }
        let mut cursor = Some(target.to_string());
        while let Some(cur) = cursor {
            if cur == id {
                return Err(AppError::Other(
                    "cannot move a collection into its own descendant".into(),
                ));
            }
            cursor = conn
                .query_row("SELECT parent_id FROM collections WHERE id = ?1", params![cur], |r| r.get(0))
                .ok();
        }
    }
    let n = conn.execute(
        "UPDATE collections SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, new_parent_id, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    get(conn, id)
}

/// Set sibling sort order to match the given id sequence (drag-drop reorder).
pub fn reorder(conn: &Connection, ids: &[String]) -> AppResult<()> {
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE collections SET sort_order = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    Ok(())
}

/// Deep-copy a collection, its nested sub-collections, and their requests.
/// The copy is inserted as a sibling of the original (same parent).
pub fn duplicate(conn: &Connection, id: &str, new_name: Option<&str>) -> AppResult<Collection> {
    let original = get(conn, id)?;
    let name = new_name
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} (copy)", original.name));
    let copy = create(
        conn,
        &original.workspace_id,
        original.parent_id.as_deref(),
        &name,
        original.description.as_deref(),
    )?;
    // Recover the real secret under the *old* owner here, in memory —
    // `update_auth` re-masks and re-stores it under the new collection's own
    // owner below, so the copy never shares a keychain entry with the original.
    let old_owner = crate::auth::owner_key("collection", &original.id);
    let auth = crate::auth::hydrate(&old_owner, original.auth)?;
    let copy = update_auth(conn, &copy.id, auth)?;
    copy_children(conn, &original.id, &copy.id)?;
    Ok(copy)
}

fn copy_children(conn: &Connection, from_id: &str, to_id: &str) -> AppResult<()> {
    let mut req_stmt = conn.prepare(
        "SELECT id, name, method, url, headers_json, query_json, body_json, options_json, auth_json FROM requests WHERE collection_id = ?1 ORDER BY sort_order ASC",
    )?;
    let requests: Vec<(String, String, String, String, String, String, String, String, String)> = req_stmt
        .query_map(params![from_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let now = now_millis();
    for (i, (old_id, name, method, url, headers_json, query_json, body_json, options_json, auth_json)) in requests.into_iter().enumerate() {
        let new_id = Uuid::new_v4().to_string();
        let old_owner = crate::auth::owner_key("request", &old_id);
        let original_auth: crate::model::auth::RequestAuth = serde_json::from_str(&auth_json).unwrap_or_default();
        let auth = crate::auth::hydrate_request_auth(&old_owner, original_auth)?;
        let new_owner = crate::auth::owner_key("request", &new_id);
        let masked_auth = crate::auth::persist_request_auth(&new_owner, auth)?;
        conn.execute(
            "INSERT INTO requests (id, collection_id, name, method, url, headers_json, query_json, body_json, options_json, auth_json, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![new_id, to_id, name, method, url, headers_json, query_json, body_json, options_json, serde_json::to_string(&masked_auth)?, i as i64, now],
        )?;
    }

    let mut child_stmt = conn.prepare("SELECT id, name, description, auth_json FROM collections WHERE parent_id = ?1 ORDER BY sort_order ASC")?;
    let children: Vec<(String, String, Option<String>, String)> = child_stmt
        .query_map(params![from_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (child_id, child_name, child_desc, child_auth_json) in children {
        let workspace_id: String = conn.query_row(
            "SELECT workspace_id FROM collections WHERE id = ?1",
            params![to_id],
            |r| r.get(0),
        )?;
        let child_copy = create(conn, &workspace_id, Some(to_id), &child_name, child_desc.as_deref())?;
        let old_owner = crate::auth::owner_key("collection", &child_id);
        let original_auth: AuthConfig = serde_json::from_str(&child_auth_json).unwrap_or_default();
        let auth = crate::auth::hydrate(&old_owner, original_auth)?;
        let child_copy = update_auth(conn, &child_copy.id, auth)?;
        copy_children(conn, &child_id, &child_copy.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::RequestAuth;
    use crate::model::SavedRequestInput;

    fn sample_input(name: &str) -> SavedRequestInput {
        SavedRequestInput {
            name: name.to_string(),
            method: "GET".to_string(),
            url: "https://a.test".to_string(),
            headers: vec![],
            query: vec![],
            body: Default::default(),
            options: Default::default(),
            auth: RequestAuth::default(),
            pre_request_script: String::new(),
            post_response_script: String::new(),
        }
    }

    /// Regression guard: collections have no `ON DELETE CASCADE` in the schema
    /// (confirmed — no `FOREIGN KEY` clause anywhere in the migrations), so
    /// deleting a non-empty collection used to leave its requests and nested
    /// sub-collections as unreachable orphans. `delete` now walks the subtree
    /// and removes all of it.
    #[test]
    fn delete_removes_nested_subcollections_and_their_requests() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let parent = create(&conn, &ws.id, None, "Parent", None).unwrap();
        let child = create(&conn, &ws.id, Some(&parent.id), "Child", None).unwrap();
        let grandchild = create(&conn, &ws.id, Some(&child.id), "Grandchild", None).unwrap();

        crate::store::requests::create(&conn, &parent.id, &sample_input("in parent")).unwrap();
        crate::store::requests::create(&conn, &grandchild.id, &sample_input("in grandchild")).unwrap();

        delete(&conn, &parent.id).unwrap();

        assert!(get(&conn, &parent.id).is_err());
        assert!(get(&conn, &child.id).is_err());
        assert!(get(&conn, &grandchild.id).is_err());
        assert_eq!(crate::store::requests::list_by_collection(&conn, &parent.id).unwrap().len(), 0);
        assert_eq!(crate::store::requests::list_by_collection(&conn, &grandchild.id).unwrap().len(), 0);
    }

    #[test]
    fn delete_leaves_siblings_untouched() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let a = create(&conn, &ws.id, None, "A", None).unwrap();
        let b = create(&conn, &ws.id, None, "B", None).unwrap();

        delete(&conn, &a.id).unwrap();

        assert!(get(&conn, &a.id).is_err());
        assert!(get(&conn, &b.id).is_ok());
    }

    #[test]
    fn delete_of_missing_id_is_not_found() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        crate::store::workspaces::ensure_default(&mut conn).unwrap();

        assert!(delete(&conn, "does-not-exist").is_err());
    }
}
