//! Saved-request repository. Header/query/body/options are stored as JSON
//! text columns and (de)serialized through the existing `model::http` types,
//! so a saved request round-trips exactly into the same `HttpRequest` shape
//! used to actually send it.

use crate::error::{AppError, AppResult};
use crate::model::http::{HeaderEntry, KeyValue, RequestBody, RequestOptions};
use crate::model::{SavedRequest, SavedRequestInput, Tag};
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

fn row_to_request(r: &rusqlite::Row) -> rusqlite::Result<(SavedRequest, String)> {
    let headers_json: String = r.get(4)?;
    let query_json: String = r.get(5)?;
    let body_json: String = r.get(6)?;
    let options_json: String = r.get(7)?;
    let collection_id: String = r.get(1)?;
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
        tags: Vec::new(),
        sort_order: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        last_used_at: r.get(12)?,
    };
    Ok((req, collection_id))
}

const SELECT: &str = "SELECT id, collection_id, name, method, headers_json, query_json, body_json, options_json, url, sort_order, created_at, updated_at, last_used_at FROM requests";

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
    conn.execute(
        "INSERT INTO requests (id, collection_id, name, method, url, headers_json, query_json, body_json, options_json, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
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
            next_order,
            now,
        ],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: &SavedRequestInput) -> AppResult<SavedRequest> {
    let n = conn.execute(
        "UPDATE requests SET name = ?2, method = ?3, url = ?4, headers_json = ?5, query_json = ?6, body_json = ?7, options_json = ?8, updated_at = ?9 WHERE id = ?1",
        params![
            id,
            input.name,
            input.method,
            input.url,
            serde_json::to_string(&input.headers)?,
            serde_json::to_string(&input.query)?,
            serde_json::to_string(&input.body)?,
            serde_json::to_string(&input.options)?,
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
    let input = SavedRequestInput {
        name: new_name.map(str::to_string).unwrap_or_else(|| format!("{} (copy)", original.name)),
        method: original.method,
        url: original.url,
        headers: original.headers,
        query: original.query,
        body: original.body,
        options: original.options,
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
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = format!("{}*", query.trim().replace('"', ""));
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
