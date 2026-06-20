//! Workspace repository — CRUD over the `workspaces` table.

use crate::error::{AppError, AppResult};
use crate::model::Workspace;
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

fn row_to_workspace(r: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: r.get(0)?,
        name: r.get(1)?,
        created_at: r.get(2)?,
        updated_at: r.get(3)?,
        is_active: r.get::<_, i64>(4)? != 0,
    })
}

const SELECT: &str = "SELECT id, name, created_at, updated_at, is_active FROM workspaces";

pub fn list(conn: &Connection) -> AppResult<Vec<Workspace>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY created_at ASC"))?;
    let rows = stmt.query_map([], row_to_workspace)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn create(conn: &Connection, name: &str) -> AppResult<Workspace> {
    let now = now_millis();
    let ws = Workspace {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        created_at: now,
        updated_at: now,
        is_active: false,
    };
    conn.execute(
        "INSERT INTO workspaces (id, name, created_at, updated_at, is_active) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![ws.id, ws.name, ws.created_at, ws.updated_at, ws.is_active as i64],
    )?;
    Ok(ws)
}

/// Mark `id` as the single active workspace.
pub fn set_active(conn: &mut Connection, id: &str) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute("UPDATE workspaces SET is_active = 0", [])?;
    let n = tx.execute(
        "UPDATE workspaces SET is_active = 1, updated_at = ?2 WHERE id = ?1",
        params![id, now_millis()],
    )?;
    tx.commit()?;
    if n == 0 {
        return Err(AppError::NotFound(format!("workspace {id}")));
    }
    Ok(())
}

/// Return the active workspace, if any.
pub fn active(conn: &Connection) -> AppResult<Option<Workspace>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE is_active = 1 LIMIT 1"))?;
    let mut rows = stmt.query_map([], row_to_workspace)?;
    match rows.next() {
        Some(w) => Ok(Some(w?)),
        None => Ok(None),
    }
}

/// Ensure at least one workspace exists and one is active. Returns the active one.
pub fn ensure_default(conn: &mut Connection) -> AppResult<Workspace> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))?;
    if count == 0 {
        let ws = create(conn, "My Workspace")?;
        set_active(conn, &ws.id)?;
    } else if active(conn)?.is_none() {
        // Workspaces exist but none active (e.g. the active one was deleted).
        let first: String =
            conn.query_row("SELECT id FROM workspaces ORDER BY created_at ASC LIMIT 1", [], |r| {
                r.get(0)
            })?;
        set_active(conn, &first)?;
    }
    active(conn)?.ok_or_else(|| AppError::Other("no active workspace after ensure_default".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db;

    #[test]
    fn migrations_create_schema_and_default_workspace() {
        let mut conn = db::open_in_memory().expect("open");
        // Fresh DB has no workspaces.
        assert!(list(&conn).unwrap().is_empty());

        // ensure_default creates and activates one.
        let ws = ensure_default(&mut conn).unwrap();
        assert_eq!(ws.name, "My Workspace");
        assert!(ws.is_active);
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn create_and_switch_active() {
        let mut conn = db::open_in_memory().expect("open");
        let a = ensure_default(&mut conn).unwrap();
        let b = create(&conn, "Second").unwrap();
        set_active(&mut conn, &b.id).unwrap();

        let act = active(&conn).unwrap().unwrap();
        assert_eq!(act.id, b.id);
        assert_ne!(act.id, a.id);
    }

    /// Exercises the real-file path the GUI uses: directory creation, on-disk
    /// WAL, and persistence across a reopen (which the in-memory tests can't).
    #[test]
    fn open_on_disk_persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("restman-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("restman.db");

        {
            let mut conn = db::open(&path).unwrap();
            ensure_default(&mut conn).unwrap();
            assert_eq!(list(&conn).unwrap().len(), 1);
        }
        assert!(path.exists());

        // Reopen: migrations are a no-op and the workspace is still there.
        {
            let conn = db::open(&path).unwrap();
            assert_eq!(list(&conn).unwrap().len(), 1);
            assert!(active(&conn).unwrap().is_some());
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
