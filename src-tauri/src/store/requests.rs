//! Saved-request repository. Header/query/body/options are stored as JSON
//! text columns and (de)serialized through the existing `model::http` types,
//! so a saved request round-trips exactly into the same `HttpRequest` shape
//! used to actually send it.

use crate::error::{AppError, AppResult};
use crate::model::auth::RequestAuth;
use crate::model::http::{HeaderEntry, KeyValue, RequestBody, RequestOptions};
use crate::model::{RequestKind, SavedRequest, SavedRequestInput, Tag};
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

fn row_to_request(r: &rusqlite::Row) -> rusqlite::Result<(SavedRequest, String)> {
    let headers_json: String = r.get(4)?;
    let query_json: String = r.get(5)?;
    let body_json: String = r.get(6)?;
    let options_json: String = r.get(7)?;
    let auth_json: String = r.get(13)?;
    let collection_id: String = r.get(1)?;
    let kind_str: String = r.get(16)?;
    let stream_config_json: Option<String> = r.get(17)?;
    let req = SavedRequest {
        id: r.get(0)?,
        collection_id: collection_id.clone(),
        name: r.get(2)?,
        method: r.get(3)?,
        url: r.get(8)?,
        headers: serde_json::from_str::<Vec<HeaderEntry>>(&headers_json).unwrap_or_default(),
        query: serde_json::from_str::<Vec<KeyValue>>(&query_json).unwrap_or_default(),
        body: serde_json::from_str::<RequestBody>(&body_json).unwrap_or_default(),
        options: serde_json::from_str::<RequestOptions>(&options_json).unwrap_or_default(),
        auth: serde_json::from_str::<RequestAuth>(&auth_json).unwrap_or_default(),
        tags: Vec::new(),
        sort_order: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        last_used_at: r.get(12)?,
        pre_request_script: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
        post_response_script: r.get::<_, Option<String>>(15)?.unwrap_or_default(),
        kind: RequestKind::from_db_str(&kind_str),
        stream_config: stream_config_json.and_then(|s| serde_json::from_str(&s).ok()),
    };
    Ok((req, collection_id))
}

const SELECT: &str = "SELECT id, collection_id, name, method, headers_json, query_json, body_json, options_json, url, sort_order, created_at, updated_at, last_used_at, auth_json, pre_request_script, post_response_script, kind, stream_config_json FROM requests";

fn attach_tags(conn: &Connection, req: &mut SavedRequest) -> AppResult<()> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.workspace_id, t.name, t.color FROM tags t
         JOIN request_tags rt ON rt.tag_id = t.id WHERE rt.request_id = ?1 ORDER BY t.name ASC",
    )?;
    let tags = stmt
        .query_map(params![req.id], |r| {
            Ok(Tag {
                id: r.get(0)?,
                workspace_id: r.get(1)?,
                name: r.get(2)?,
                color: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    req.tags = tags;
    Ok(())
}

pub fn list_by_collection(conn: &Connection, collection_id: &str) -> AppResult<Vec<SavedRequest>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE collection_id = ?1 ORDER BY sort_order ASC, created_at ASC"))?;
    let rows = stmt.query_map(params![collection_id], row_to_request)?;
    let mut out = Vec::new();
    for row in rows {
        let (mut req, _) = row?;
        attach_tags(conn, &mut req)?;
        out.push(req);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<SavedRequest> {
    let (mut req, _) = conn
        .query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_request)
        .map_err(|_| AppError::NotFound(format!("request {id}")))?;
    attach_tags(conn, &mut req)?;
    Ok(req)
}

pub fn create(conn: &Connection, collection_id: &str, input: &SavedRequestInput) -> AppResult<SavedRequest> {
    let now = now_millis();
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM requests WHERE collection_id = ?1",
        params![collection_id],
        |r| r.get(0),
    )?;
    let id = Uuid::new_v4().to_string();
    let owner = crate::auth::owner_key("request", &id);
    let auth = crate::auth::persist_request_auth(&owner, input.auth.clone())?;
    conn.execute(
        "INSERT INTO requests (id, collection_id, name, method, url, headers_json, query_json, body_json, options_json, auth_json, pre_request_script, post_response_script, kind, stream_config_json, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![
            id,
            collection_id,
            input.name,
            input.method,
            input.url,
            serde_json::to_string(&input.headers)?,
            serde_json::to_string(&input.query)?,
            serde_json::to_string(&input.body)?,
            serde_json::to_string(&input.options)?,
            serde_json::to_string(&auth)?,
            input.pre_request_script,
            input.post_response_script,
            input.kind.as_db_str(),
            input.stream_config.as_ref().map(serde_json::to_string).transpose()?,
            next_order,
            now,
        ],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: &SavedRequestInput) -> AppResult<SavedRequest> {
    let owner = crate::auth::owner_key("request", id);
    let auth = crate::auth::persist_request_auth(&owner, input.auth.clone())?;
    let n = conn.execute(
        "UPDATE requests SET name = ?2, method = ?3, url = ?4, headers_json = ?5, query_json = ?6, body_json = ?7, options_json = ?8, auth_json = ?9, pre_request_script = ?10, post_response_script = ?11, kind = ?12, stream_config_json = ?13, updated_at = ?14 WHERE id = ?1",
        params![
            id,
            input.name,
            input.method,
            input.url,
            serde_json::to_string(&input.headers)?,
            serde_json::to_string(&input.query)?,
            serde_json::to_string(&input.body)?,
            serde_json::to_string(&input.options)?,
            serde_json::to_string(&auth)?,
            input.pre_request_script,
            input.post_response_script,
            input.kind.as_db_str(),
            input.stream_config.as_ref().map(serde_json::to_string).transpose()?,
            now_millis(),
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("request {id}")));
    }
    get(conn, id)
}

pub fn touch_last_used(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE requests SET last_used_at = ?2 WHERE id = ?1",
        params![id, now_millis()],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM requests WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("request {id}")));
    }
    crate::auth::sweep(&crate::auth::owner_key("request", id));
    Ok(())
}

pub fn move_to(conn: &Connection, id: &str, collection_id: &str) -> AppResult<SavedRequest> {
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM requests WHERE collection_id = ?1",
        params![collection_id],
        |r| r.get(0),
    )?;
    let n = conn.execute(
        "UPDATE requests SET collection_id = ?2, sort_order = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, collection_id, next_order, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("request {id}")));
    }
    get(conn, id)
}

pub fn reorder(conn: &Connection, ids: &[String]) -> AppResult<()> {
    for (i, id) in ids.iter().enumerate() {
        conn.execute("UPDATE requests SET sort_order = ?2 WHERE id = ?1", params![id, i as i64])?;
    }
    Ok(())
}

pub fn duplicate(conn: &Connection, id: &str, new_name: Option<&str>) -> AppResult<SavedRequest> {
    let original = get(conn, id)?;
    // Recover the real secret under the *old* owner here, in memory — `create`
    // re-masks and re-stores it under the new request's own owner below, so
    // the copy never shares a keychain entry with the original.
    let old_owner = crate::auth::owner_key("request", &original.id);
    let auth = crate::auth::hydrate_request_auth(&old_owner, original.auth)?;
    let input = SavedRequestInput {
        name: new_name.map(str::to_string).unwrap_or_else(|| format!("{} (copy)", original.name)),
        method: original.method,
        url: original.url,
        headers: original.headers,
        query: original.query,
        body: original.body,
        options: original.options,
        auth,
        pre_request_script: original.pre_request_script,
        post_response_script: original.post_response_script,
        kind: original.kind,
        stream_config: original.stream_config,
    };
    create(conn, &original.collection_id, &input)
}

pub fn set_tags(conn: &Connection, request_id: &str, tag_ids: &[String]) -> AppResult<()> {
    conn.execute("DELETE FROM request_tags WHERE request_id = ?1", params![request_id])?;
    for tag_id in tag_ids {
        conn.execute(
            "INSERT OR IGNORE INTO request_tags (request_id, tag_id) VALUES (?1, ?2)",
            params![request_id, tag_id],
        )?;
    }
    Ok(())
}

/// One match from `search`: the request plus highlight-marked snippets.
/// Matched text is wrapped in `\u{1}`...`\u{2}` sentinels (never raw HTML),
/// so the frontend can split on them and render `<mark>` safely.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub request: SavedRequest,
    pub name_highlight: String,
    pub url_highlight: String,
}

pub fn search(conn: &Connection, workspace_id: &str, query: &str, method: Option<&str>) -> AppResult<Vec<SearchHit>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return list_all(conn, workspace_id, method);
    }
    let fts_query = format!("{}*", trimmed.replace('"', ""));
    let mut stmt = conn.prepare(
        "SELECT r.id, highlight(requests_fts, 0, char(1), char(2)), highlight(requests_fts, 1, char(1), char(2))
         FROM requests_fts
         JOIN requests r ON r.rowid = requests_fts.rowid
         JOIN collections c ON c.id = r.collection_id
         WHERE requests_fts MATCH ?1 AND c.workspace_id = ?2
         ORDER BY rank LIMIT 50",
    )?;
    let rows = stmt.query_map(params![fts_query, workspace_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name_highlight, url_highlight) = row?;
        let request = get(conn, &id)?;
        if let Some(m) = method {
            if !request.method.eq_ignore_ascii_case(m) {
                continue;
            }
        }
        out.push(SearchHit { request, name_highlight, url_highlight });
    }
    Ok(out)
}

/// `search` with no query text: tag/method filtering on saved requests
/// shouldn't require typing a search term first. There's no FTS match, so
/// highlights are just the plain name/url (no sentinel markers to render).
fn list_all(conn: &Connection, workspace_id: &str, method: Option<&str>) -> AppResult<Vec<SearchHit>> {
    let mut stmt = conn.prepare(
        "SELECT r.id FROM requests r
         JOIN collections c ON c.id = r.collection_id
         WHERE c.workspace_id = ?1 AND (?2 IS NULL OR UPPER(r.method) = UPPER(?2))
         ORDER BY r.name ASC LIMIT 50",
    )?;
    let ids = stmt
        .query_map(params![workspace_id, method], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for id in ids {
        let request = get(conn, &id)?;
        out.push(SearchHit {
            name_highlight: request.name.clone(),
            url_highlight: request.url.clone(),
            request,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(name: &str, method: &str) -> SavedRequestInput {
        SavedRequestInput {
            name: name.to_string(),
            method: method.to_string(),
            url: "https://example.com".to_string(),
            headers: Vec::new(),
            query: Vec::new(),
            body: RequestBody::default(),
            options: RequestOptions::default(),
            auth: RequestAuth::default(),
            pre_request_script: String::new(),
            post_response_script: String::new(),
            kind: RequestKind::default(),
            stream_config: None,
        }
    }

    #[test]
    fn search_with_blank_query_lists_all_and_respects_method_filter() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let collection = crate::store::collections::create(&conn, &ws.id, None, "C", None).unwrap();

        create(&conn, &collection.id, &make_input("Get one", "GET")).unwrap();
        create(&conn, &collection.id, &make_input("Post one", "POST")).unwrap();

        let all = search(&conn, &ws.id, "", None).unwrap();
        assert_eq!(all.len(), 2);

        let gets = search(&conn, &ws.id, "", Some("GET")).unwrap();
        assert_eq!(gets.len(), 1);
        assert_eq!(gets[0].request.name, "Get one");
    }

    #[test]
    fn search_with_blank_query_and_no_matches_returns_empty() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let collection = crate::store::collections::create(&conn, &ws.id, None, "C", None).unwrap();
        create(&conn, &collection.id, &make_input("Get one", "GET")).unwrap();

        let posts = search(&conn, &ws.id, "", Some("POST")).unwrap();
        assert!(posts.is_empty());
    }
}
