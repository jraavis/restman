//! Variable repository. Each row belongs to exactly one scope (global,
//! workspace, collection, or environment) — see the CHECK constraint in the
//! v2 migration. Resolution across scopes lives in `crate::vars`.

use crate::error::{AppError, AppResult};
use crate::model::{VarScope, VarType, Variable, VariableInput};
use crate::secrets;
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

const SELECT: &str =
    "SELECT id, workspace_id, collection_id, environment_id, key, value, var_type, is_secret, enabled, sort_order, created_at, updated_at FROM variables";

fn row_to_variable(r: &rusqlite::Row) -> rusqlite::Result<Variable> {
    let workspace_id: Option<String> = r.get(1)?;
    let collection_id: Option<String> = r.get(2)?;
    let environment_id: Option<String> = r.get(3)?;
    let scope = if let Some(id) = environment_id {
        VarScope::Environment(id)
    } else if let Some(id) = collection_id {
        VarScope::Collection(id)
    } else if let Some(id) = workspace_id {
        VarScope::Workspace(id)
    } else {
        VarScope::Global
    };
    let var_type: String = r.get(6)?;
    Ok(Variable {
        id: r.get(0)?,
        scope,
        key: r.get(4)?,
        value: r.get(5)?,
        var_type: VarType::parse(&var_type),
        is_secret: r.get::<_, i64>(7)? != 0,
        enabled: r.get::<_, i64>(8)? != 0,
        sort_order: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

fn scope_columns(scope: &VarScope) -> (Option<&str>, Option<&str>, Option<&str>) {
    match scope {
        VarScope::Global => (None, None, None),
        VarScope::Workspace(id) => (Some(id.as_str()), None, None),
        VarScope::Collection(id) => (None, Some(id.as_str()), None),
        VarScope::Environment(id) => (None, None, Some(id.as_str())),
    }
}

pub fn list(conn: &Connection, scope: &VarScope) -> AppResult<Vec<Variable>> {
    let (workspace_id, collection_id, environment_id) = scope_columns(scope);
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE workspace_id IS ?1 AND collection_id IS ?2 AND environment_id IS ?3 ORDER BY sort_order ASC, created_at ASC"
    ))?;
    let rows = stmt.query_map(params![workspace_id, collection_id, environment_id], row_to_variable)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn create(conn: &Connection, scope: &VarScope, input: &VariableInput) -> AppResult<Variable> {
    let (workspace_id, collection_id, environment_id) = scope_columns(scope);
    let now = now_millis();
    let id = Uuid::new_v4().to_string();
    // Secret values never land in the `value` column — write to the
    // keychain first so a backend failure (e.g. no Secret Service on
    // Linux) aborts before any row exists, rather than leaving a half-saved
    // variable with no recoverable value anywhere.
    let stored_value = if input.is_secret {
        secrets::set(&id, &input.value)?;
        String::new()
    } else {
        input.value.clone()
    };
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM variables WHERE workspace_id IS ?1 AND collection_id IS ?2 AND environment_id IS ?3",
        params![workspace_id, collection_id, environment_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO variables (id, workspace_id, collection_id, environment_id, key, value, var_type, is_secret, enabled, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            workspace_id,
            collection_id,
            environment_id,
            input.key,
            stored_value,
            input.var_type.as_str(),
            input.is_secret as i64,
            input.enabled as i64,
            next_order,
            now,
        ],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Variable> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_variable)
        .map_err(|_| AppError::NotFound(format!("variable {id}")))
}

pub fn update(conn: &Connection, id: &str, input: &VariableInput) -> AppResult<Variable> {
    let stored_value = if input.is_secret {
        secrets::set(id, &input.value)?;
        String::new()
    } else {
        // Not (or no longer) a secret — drop any stale keychain entry from
        // a prior secret state rather than leaving an orphaned credential.
        let _ = secrets::delete(id);
        input.value.clone()
    };
    let n = conn.execute(
        "UPDATE variables SET key = ?2, value = ?3, var_type = ?4, is_secret = ?5, enabled = ?6, updated_at = ?7 WHERE id = ?1",
        params![
            id,
            input.key,
            stored_value,
            input.var_type.as_str(),
            input.is_secret as i64,
            input.enabled as i64,
            now_millis(),
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("variable {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM variables WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("variable {id}")));
    }
    let _ = secrets::delete(id);
    Ok(())
}

/// One-time sweep moving any pre-keychain plaintext secret values into the
/// OS credential store. Idempotent by construction: a migrated row's
/// `value` column is empty, so the `WHERE` clause matches nothing on later
/// launches — no separate "has this run" flag needed. Errors are swallowed
/// rather than propagated because this runs at app startup; a keychain
/// that's unavailable (e.g. Linux with no Secret Service daemon) must not
/// block the app from opening. Affected rows simply stay plaintext and are
/// retried on the next launch.
pub fn migrate_plaintext_secrets_to_keychain(conn: &Connection) {
    let rows: rusqlite::Result<Vec<(String, String)>> = (|| {
        conn.prepare("SELECT id, value FROM variables WHERE is_secret = 1 AND value != ''")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    })();
    let Ok(rows) = rows else { return };
    for (id, value) in rows {
        if secrets::set(&id, &value).is_ok() {
            let _ = conn.execute("UPDATE variables SET value = '' WHERE id = ?1", params![id]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VarType;

    fn input(key: &str, value: &str, is_secret: bool) -> VariableInput {
        VariableInput {
            key: key.into(),
            value: value.into(),
            var_type: VarType::String,
            is_secret,
            enabled: true,
        }
    }

    #[test]
    fn secret_value_never_lands_in_the_value_column() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let v = create(&conn, &VarScope::Workspace(ws.id.clone()), &input("token", "tok_abc123", true)).unwrap();
        assert_eq!(v.value, "");
        assert_eq!(secrets::get(&v.id).unwrap().unwrap(), "tok_abc123");
    }

    #[test]
    fn update_preserves_keychain_value_when_input_unchanged() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let v = create(&conn, &VarScope::Workspace(ws.id.clone()), &input("token", "tok_abc123", true)).unwrap();
        let updated = update(&conn, &v.id, &input("token_renamed", "tok_abc123", true)).unwrap();
        assert_eq!(updated.key, "token_renamed");
        assert_eq!(secrets::get(&v.id).unwrap().unwrap(), "tok_abc123");
    }

    #[test]
    fn switching_off_secret_clears_keychain_entry() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let v = create(&conn, &VarScope::Workspace(ws.id.clone()), &input("token", "tok_abc123", true)).unwrap();
        let updated = update(&conn, &v.id, &input("token", "plain_now", false)).unwrap();
        assert_eq!(updated.value, "plain_now");
        assert_eq!(secrets::get(&v.id).unwrap(), None);
    }

    #[test]
    fn delete_cleans_up_keychain_entry() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        let v = create(&conn, &VarScope::Workspace(ws.id.clone()), &input("token", "tok_abc123", true)).unwrap();
        delete(&conn, &v.id).unwrap();
        assert_eq!(secrets::get(&v.id).unwrap(), None);
    }

    #[test]
    fn migrate_moves_plaintext_secret_into_keychain_and_clears_column() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        // Simulate a pre-keychain row by inserting directly with a plaintext
        // value in a secret row, bypassing `create` (which would already
        // route it through the keychain).
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO variables (id, workspace_id, collection_id, environment_id, key, value, var_type, is_secret, enabled, sort_order, created_at, updated_at)
             VALUES (?1, ?2, NULL, NULL, 'legacy', 'still_plaintext', 'string', 1, 1, 0, 0, 0)",
            params![id, ws.id],
        )
        .unwrap();

        migrate_plaintext_secrets_to_keychain(&conn);

        let row = get(&conn, &id).unwrap();
        assert_eq!(row.value, "");
        assert_eq!(secrets::get(&id).unwrap().unwrap(), "still_plaintext");
    }
}
