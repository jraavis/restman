//! Tag repository — color-coded labels attached to requests via `request_tags`.

use crate::error::{AppError, AppResult};
use crate::model::Tag;
use rusqlite::{params, Connection};
use uuid::Uuid;

fn row_to_tag(r: &rusqlite::Row) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        color: r.get(3)?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> AppResult<Vec<Tag>> {
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, color FROM tags WHERE workspace_id = ?1 ORDER BY name ASC")?;
    let rows = stmt.query_map(params![workspace_id], row_to_tag)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn create(conn: &Connection, workspace_id: &str, name: &str, color: &str) -> AppResult<Tag> {
    let tag = Tag {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        name: name.to_string(),
        color: color.to_string(),
    };
    conn.execute(
        "INSERT INTO tags (id, workspace_id, name, color) VALUES (?1, ?2, ?3, ?4)",
        params![tag.id, tag.workspace_id, tag.name, tag.color],
    )?;
    Ok(tag)
}

pub fn update(conn: &Connection, id: &str, name: &str, color: &str) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE tags SET name = ?2, color = ?3 WHERE id = ?1",
        params![id, name, color],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("tag {id}")));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("tag {id}")));
    }
    Ok(())
}
