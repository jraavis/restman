//! Environment repository. At most one environment is active per workspace
//! at a time (same invariant as `Workspace::is_active`, scoped narrower).

use crate::error::{AppError, AppResult};
use crate::model::Environment;
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

const SELECT: &str =
    "SELECT id, workspace_id, collection_id, name, group_name, is_active, created_at, updated_at FROM environments";

fn row_to_env(r: &rusqlite::Row) -> rusqlite::Result<Environment> {
    Ok(Environment {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        collection_id: r.get(2)?,
        name: r.get(3)?,
        group_name: r.get(4)?,
        is_active: r.get::<_, i64>(5)? != 0,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> AppResult<Vec<Environment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE workspace_id = ?1 ORDER BY created_at ASC"))?;
    let rows = stmt.query_map(params![workspace_id], row_to_env)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Environment> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_env)
        .map_err(|_| AppError::NotFound(format!("environment {id}")))
}

pub fn create(
    conn: &Connection,
    workspace_id: &str,
    collection_id: Option<&str>,
    name: &str,
    group_name: Option<&str>,
) -> AppResult<Environment> {
    let now = now_millis();
    let env = Environment {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        collection_id: collection_id.map(str::to_string),
        name: name.to_string(),
        group_name: group_name.map(str::to_string),
        is_active: false,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO environments (id, workspace_id, collection_id, name, group_name, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
        params![env.id, env.workspace_id, env.collection_id, env.name, env.group_name, env.created_at],
    )?;
    Ok(env)
}

pub fn update(conn: &Connection, id: &str, name: &str, group_name: Option<&str>) -> AppResult<Environment> {
    let n = conn.execute(
        "UPDATE environments SET name = ?2, group_name = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, group_name, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM environments WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    Ok(())
}

/// Activate `id` as the one active environment for its workspace (deactivating
/// any other). Pass `id = None` to clear the active environment entirely.
pub fn set_active(conn: &mut Connection, workspace_id: &str, id: Option<&str>) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute("UPDATE environments SET is_active = 0 WHERE workspace_id = ?1", params![workspace_id])?;
    if let Some(id) = id {
        let n = tx.execute(
            "UPDATE environments SET is_active = 1, updated_at = ?3 WHERE id = ?1 AND workspace_id = ?2",
            params![id, workspace_id, now_millis()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("environment {id}")));
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn active_for_workspace(conn: &Connection, workspace_id: &str) -> AppResult<Option<Environment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE workspace_id = ?1 AND is_active = 1 LIMIT 1"))?;
    let mut rows = stmt.query_map(params![workspace_id], row_to_env)?;
    match rows.next() {
        Some(e) => Ok(Some(e?)),
        None => Ok(None),
    }
}
