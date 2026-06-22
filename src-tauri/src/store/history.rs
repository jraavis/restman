//! History repository: full request/response snapshots, auto-trimmed to a
//! configurable retention count read from the `settings` table.

use crate::error::AppResult;
use crate::model::history::{HistoryEntry, HistoryFilter};
use crate::model::http::{HttpRequest, HttpResponse};
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

const DEFAULT_RETENTION: i64 = 500;
const SELECT: &str = "SELECT id, workspace_id, request_id, name, method, url, status, duration_ms, request_json, response_json, error, created_at FROM history";

fn row_to_entry(r: &rusqlite::Row) -> rusqlite::Result<HistoryEntry> {
    let request_json: String = r.get(8)?;
    let response_json: Option<String> = r.get(9)?;
    Ok(HistoryEntry {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        request_id: r.get(2)?,
        name: r.get(3)?,
        method: r.get(4)?,
        url: r.get(5)?,
        status: r.get::<_, Option<i64>>(6)?.map(|v| v as u16),
        duration_ms: r.get(7)?,
        request: serde_json::from_str::<HttpRequest>(&request_json).unwrap_or_default(),
        response: response_json.and_then(|j| serde_json::from_str::<HttpResponse>(&j).ok()),
        error: r.get(10)?,
        created_at: r.get(11)?,
    })
}

pub fn insert(
    conn: &Connection,
    workspace_id: &str,
    request_id: Option<&str>,
    name: &str,
    request: &HttpRequest,
    response: Option<&HttpResponse>,
    error: Option<&str>,
) -> AppResult<HistoryEntry> {
    let id = Uuid::new_v4().to_string();
    let now = now_millis();
    let response_json = match response {
        Some(r) => Some(serde_json::to_string(r)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO history (id, workspace_id, request_id, name, method, url, status, duration_ms, request_json, response_json, error, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            workspace_id,
            request_id,
            name,
            request.method,
            request.url,
            response.map(|r| r.status as i64),
            response.map(|r| r.timing.total_ms),
            serde_json::to_string(request)?,
            response_json,
            error,
            now,
        ],
    )?;
    enforce_retention(conn, workspace_id)?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<HistoryEntry> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_entry)
        .map_err(|_| crate::error::AppError::NotFound(format!("history entry {id}")))
}

pub fn list(conn: &Connection, workspace_id: &str, filter: &HistoryFilter) -> AppResult<Vec<HistoryEntry>> {
    let mut sql = format!("{SELECT} WHERE workspace_id = ?1");
    if filter.method.is_some() {
        sql.push_str(" AND method = ?2");
    }
    if filter.status_min.is_some() {
        sql.push_str(" AND status >= ?3");
    }
    if filter.status_max.is_some() {
        sql.push_str(" AND status <= ?4");
    }
    if filter.text.is_some() {
        sql.push_str(" AND (name LIKE ?5 OR url LIKE ?5)");
    }
    if filter.date_min.is_some() {
        sql.push_str(" AND created_at >= ?6");
    }
    if filter.date_max.is_some() {
        sql.push_str(" AND created_at <= ?7");
    }
    // `rowid` tiebreaker: millisecond-resolution `created_at` ties on rapid
    // successive inserts otherwise leave order unspecified.
    sql.push_str(" ORDER BY created_at DESC, rowid DESC LIMIT ?8");

    let mut stmt = conn.prepare(&sql)?;
    let text_pattern = filter.text.as_ref().map(|t| format!("%{t}%"));
    let rows = stmt.query_map(
        params![
            workspace_id,
            filter.method,
            filter.status_min.map(|v| v as i64),
            filter.status_max.map(|v| v as i64),
            text_pattern,
            filter.date_min,
            filter.date_max,
            filter.limit.unwrap_or(200),
        ],
        row_to_entry,
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection, workspace_id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM history WHERE workspace_id = ?1", params![workspace_id])?;
    Ok(())
}

/// Current retention count, as exposed to the UI (Settings → History).
pub fn get_retention(conn: &Connection) -> i64 {
    retention_limit(conn)
}

/// Set the retention count and immediately enforce it for every workspace —
/// otherwise a lowered limit would only take effect on each workspace's next
/// insert, leaving stale-but-now-over-limit entries visible until then.
pub fn set_retention(conn: &Connection, count: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('history_retention_count', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1",
        params![count.to_string()],
    )?;
    let mut stmt = conn.prepare("SELECT DISTINCT workspace_id FROM history")?;
    let workspace_ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for workspace_id in workspace_ids {
        enforce_retention(conn, &workspace_id)?;
    }
    Ok(())
}

fn retention_limit(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'history_retention_count'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<i64>().ok())
    .unwrap_or(DEFAULT_RETENTION)
}

fn enforce_retention(conn: &Connection, workspace_id: &str) -> AppResult<()> {
    let limit = retention_limit(conn);
    conn.execute(
        "DELETE FROM history WHERE workspace_id = ?1 AND id NOT IN (
            SELECT id FROM history WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2
        )",
        params![workspace_id, limit],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::http::{HttpResponse, Timing};

    fn sample_request(method: &str, url: &str) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            url: url.to_string(),
            ..Default::default()
        }
    }

    fn sample_response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
            status_text: "OK".to_string(),
            headers: vec![],
            body_base64: String::new(),
            size_bytes: 0,
            timing: Timing::default(),
            final_url: String::new(),
            http_version: "HTTP/1.1".to_string(),
        }
    }

    #[test]
    fn list_with_no_filter_returns_all_for_workspace_newest_first() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        insert(&conn, &ws.id, None, "first", &sample_request("GET", "https://a.test"), Some(&sample_response(200)), None).unwrap();
        insert(&conn, &ws.id, None, "second", &sample_request("POST", "https://b.test"), Some(&sample_response(201)), None).unwrap();

        let entries = list(&conn, &ws.id, &HistoryFilter::default()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "second");
        assert_eq!(entries[1].name, "first");
    }

    #[test]
    fn list_filters_by_method() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        insert(&conn, &ws.id, None, "get-one", &sample_request("GET", "https://a.test"), Some(&sample_response(200)), None).unwrap();
        insert(&conn, &ws.id, None, "post-one", &sample_request("POST", "https://b.test"), Some(&sample_response(200)), None).unwrap();

        let filter = HistoryFilter { method: Some("POST".to_string()), ..Default::default() };
        let entries = list(&conn, &ws.id, &filter).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "post-one");
    }

    /// Regression guard for the optional-clause SQL builder in `list`: when only
    /// `status_min` is set (and `status_max` is not), the generated SQL references
    /// `?3` but not `?4`. SQLite numbers placeholders by the highest index used
    /// anywhere in the text, so binding all 6 positional values is still valid even
    /// though `?4`'s bind site doesn't appear — this test would fail with a
    /// rusqlite range error if that assumption were wrong.
    #[test]
    fn list_filters_by_status_min_only_without_status_max() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        insert(&conn, &ws.id, None, "ok", &sample_request("GET", "https://a.test"), Some(&sample_response(200)), None).unwrap();
        insert(&conn, &ws.id, None, "server-error", &sample_request("GET", "https://b.test"), Some(&sample_response(500)), None).unwrap();

        let filter = HistoryFilter { status_min: Some(400), ..Default::default() };
        let entries = list(&conn, &ws.id, &filter).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "server-error");
    }

    #[test]
    fn list_filters_by_text_match_on_name_or_url() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        insert(&conn, &ws.id, None, "fetch users", &sample_request("GET", "https://api.test/users"), Some(&sample_response(200)), None).unwrap();
        insert(&conn, &ws.id, None, "fetch orders", &sample_request("GET", "https://api.test/orders"), Some(&sample_response(200)), None).unwrap();

        let filter = HistoryFilter { text: Some("orders".to_string()), ..Default::default() };
        let entries = list(&conn, &ws.id, &filter).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "fetch orders");
    }

    #[test]
    fn list_respects_limit() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        for i in 0..5 {
            insert(&conn, &ws.id, None, &format!("req-{i}"), &sample_request("GET", "https://a.test"), Some(&sample_response(200)), None).unwrap();
        }

        let filter = HistoryFilter { limit: Some(2), ..Default::default() };
        let entries = list(&conn, &ws.id, &filter).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn delete_and_clear_remove_entries() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let a = insert(&conn, &ws.id, None, "a", &sample_request("GET", "https://a.test"), Some(&sample_response(200)), None).unwrap();
        insert(&conn, &ws.id, None, "b", &sample_request("GET", "https://b.test"), Some(&sample_response(200)), None).unwrap();

        delete(&conn, &a.id).unwrap();
        assert_eq!(list(&conn, &ws.id, &HistoryFilter::default()).unwrap().len(), 1);

        clear(&conn, &ws.id).unwrap();
        assert_eq!(list(&conn, &ws.id, &HistoryFilter::default()).unwrap().len(), 0);
    }

    #[test]
    fn insert_records_error_branch_with_no_response() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();

        let entry = insert(&conn, &ws.id, None, "failed", &sample_request("GET", "https://a.test"), None, Some("connection refused")).unwrap();
        assert_eq!(entry.status, None);
        assert_eq!(entry.duration_ms, None);
        assert_eq!(entry.error.as_deref(), Some("connection refused"));
    }
}
