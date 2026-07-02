//! Tab repository. `draft_json` snapshots the live (possibly unsaved) editor
//! state per tab so closing/restarting the app doesn't lose in-progress edits.

use crate::error::AppResult;
use crate::model::http::HttpRequest;
use crate::model::Tab;
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

const SELECT: &str = "SELECT id, workspace_id, request_id, title, draft_json, sort_order, is_active, created_at, updated_at FROM tabs";

fn row_to_tab(r: &rusqlite::Row) -> rusqlite::Result<Tab> {
    let draft_json: String = r.get(4)?;
    Ok(Tab {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        request_id: r.get(2)?,
        title: r.get(3)?,
        draft: serde_json::from_str::<HttpRequest>(&draft_json).unwrap_or_default(),
        sort_order: r.get(5)?,
        is_active: r.get::<_, i64>(6)? != 0,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> AppResult<Vec<Tab>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE workspace_id = ?1 ORDER BY sort_order ASC, created_at ASC"))?;
    let rows = stmt.query_map(params![workspace_id], row_to_tab)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Tab> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_tab)
        .map_err(|_| crate::error::AppError::NotFound(format!("tab {id}")))
}

/// Create a new tab and make it the active one.
pub fn create(
    conn: &mut Connection,
    workspace_id: &str,
    request_id: Option<&str>,
    title: &str,
    draft: &HttpRequest,
) -> AppResult<Tab> {
    let now = now_millis();
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM tabs WHERE workspace_id = ?1",
        params![workspace_id],
        |r| r.get(0),
    )?;
    let id = Uuid::new_v4().to_string();
    let tx = conn.transaction()?;
    tx.execute("UPDATE tabs SET is_active = 0 WHERE workspace_id = ?1", params![workspace_id])?;
    tx.execute(
        "INSERT INTO tabs (id, workspace_id, request_id, title, draft_json, sort_order, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
        params![id, workspace_id, request_id, title, serde_json::to_string(draft)?, next_order, now],
    )?;
    tx.commit()?;
    get(conn, &id)
}

pub fn update_draft(conn: &Connection, id: &str, title: &str, draft: &HttpRequest) -> AppResult<Tab> {
    conn.execute(
        "UPDATE tabs SET title = ?2, draft_json = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, serde_json::to_string(draft)?, now_millis()],
    )?;
    get(conn, id)
}

/// Replace the persisted draft of every tab linked to `request_id`. For
/// flows where the *saved request* is the source of truth and any open tab
/// is stale — e.g. an import that just overwrote the request in place. The
/// normal editing flow is the opposite direction (tab draft → `update_draft`)
/// and must not use this.
pub fn refresh_drafts_for_request(conn: &Connection, request_id: &str, draft: &HttpRequest) -> AppResult<()> {
    conn.execute(
        "UPDATE tabs SET draft_json = ?2, updated_at = ?3 WHERE request_id = ?1",
        params![request_id, serde_json::to_string(draft)?, now_millis()],
    )?;
    Ok(())
}

/// Link a tab to the request it was just saved as (so future saves overwrite
/// rather than re-creating a request).
pub fn set_request_id(conn: &Connection, id: &str, request_id: &str) -> AppResult<Tab> {
    conn.execute(
        "UPDATE tabs SET request_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, request_id, now_millis()],
    )?;
    get(conn, id)
}

pub fn set_active(conn: &mut Connection, workspace_id: &str, id: &str) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute("UPDATE tabs SET is_active = 0 WHERE workspace_id = ?1", params![workspace_id])?;
    tx.execute("UPDATE tabs SET is_active = 1 WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(())
}

pub fn reorder(conn: &Connection, ids: &[String]) -> AppResult<()> {
    for (i, id) in ids.iter().enumerate() {
        conn.execute("UPDATE tabs SET sort_order = ?2 WHERE id = ?1", params![id, i as i64])?;
    }
    Ok(())
}

/// Close one tab. If it was active, activates the next-most-recent
/// remaining tab (if any) so the UI always has something focused.
pub fn close(conn: &mut Connection, workspace_id: &str, id: &str) -> AppResult<()> {
    let tx = conn.transaction()?;
    let was_active: i64 = tx.query_row("SELECT is_active FROM tabs WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or(0);
    tx.execute("DELETE FROM tabs WHERE id = ?1", params![id])?;
    if was_active != 0 {
        let next: Option<String> = tx
            .query_row(
                "SELECT id FROM tabs WHERE workspace_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                params![workspace_id],
                |r| r.get(0),
            )
            .ok();
        if let Some(next_id) = next {
            tx.execute("UPDATE tabs SET is_active = 1 WHERE id = ?1", params![next_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn close_others(conn: &Connection, workspace_id: &str, keep_id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM tabs WHERE workspace_id = ?1 AND id != ?2",
        params![workspace_id, keep_id],
    )?;
    conn.execute("UPDATE tabs SET is_active = 1 WHERE id = ?1", params![keep_id])?;
    Ok(())
}

pub fn close_all(conn: &Connection, workspace_id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM tabs WHERE workspace_id = ?1", params![workspace_id])?;
    Ok(())
}
