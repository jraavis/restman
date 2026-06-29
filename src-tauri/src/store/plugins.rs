//! Plugin repository — user-authored JS plugins (custom code-generators,
//! custom import/export formats), many rows per workspace.
//!
//! Not yet called outside its own tests: `commands::plugins` (the IPC layer
//! that will consume this) is a later sequential task. Suppress dead-code
//! warnings until that lands.
#![allow(dead_code)]

use crate::error::{AppError, AppResult};
use crate::model::{Plugin, PluginInput, PluginKind};
use crate::util::now_millis;
use rusqlite::{params, Connection};
use uuid::Uuid;

const SELECT: &str =
    "SELECT id, workspace_id, name, kind, language_label, source, enabled, created_at, updated_at FROM plugins";

fn kind_to_str(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Codegen => "codegen",
        PluginKind::Import => "import",
        PluginKind::Export => "export",
    }
}

fn kind_from_str(s: &str) -> AppResult<PluginKind> {
    match s {
        "codegen" => Ok(PluginKind::Codegen),
        "import" => Ok(PluginKind::Import),
        "export" => Ok(PluginKind::Export),
        other => Err(AppError::Other(format!("unknown plugin kind: {other}"))),
    }
}

fn row_to_plugin(r: &rusqlite::Row) -> rusqlite::Result<(Plugin, String)> {
    let kind_str: String = r.get(3)?;
    Ok((
        Plugin {
            id: r.get(0)?,
            workspace_id: r.get(1)?,
            name: r.get(2)?,
            kind: PluginKind::Codegen, // placeholder, fixed up by caller
            language_label: r.get(4)?,
            source: r.get(5)?,
            enabled: r.get::<_, i64>(6)? != 0,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        },
        kind_str,
    ))
}

fn finish_row(pair: rusqlite::Result<(Plugin, String)>) -> AppResult<Plugin> {
    let (mut plugin, kind_str) = pair?;
    plugin.kind = kind_from_str(&kind_str)?;
    Ok(plugin)
}

pub fn list_by_workspace(
    conn: &Connection,
    workspace_id: &str,
    kind: Option<PluginKind>,
) -> AppResult<Vec<Plugin>> {
    let plugins = match kind {
        Some(kind) => {
            let mut stmt =
                conn.prepare(&format!("{SELECT} WHERE workspace_id = ?1 AND kind = ?2 ORDER BY name ASC"))?;
            let rows = stmt.query_map(params![workspace_id, kind_to_str(kind)], row_to_plugin)?;
            rows.map(finish_row).collect::<AppResult<Vec<_>>>()?
        }
        None => {
            let mut stmt = conn.prepare(&format!("{SELECT} WHERE workspace_id = ?1 ORDER BY name ASC"))?;
            let rows = stmt.query_map(params![workspace_id], row_to_plugin)?;
            rows.map(finish_row).collect::<AppResult<Vec<_>>>()?
        }
    };
    Ok(plugins)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Plugin> {
    let pair = conn
        .query_row(&format!("{SELECT} WHERE id = ?1"), params![id], row_to_plugin)
        .map_err(|_| AppError::NotFound(format!("plugin {id}")))?;
    finish_row(Ok(pair))
}

pub fn create(conn: &Connection, workspace_id: &str, input: &PluginInput) -> AppResult<Plugin> {
    let now = now_millis();
    let plugin = Plugin {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        name: input.name.clone(),
        kind: input.kind,
        language_label: input.language_label.clone(),
        source: input.source.clone(),
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO plugins (id, workspace_id, name, kind, language_label, source, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            plugin.id,
            plugin.workspace_id,
            plugin.name,
            kind_to_str(plugin.kind),
            plugin.language_label,
            plugin.source,
            plugin.enabled as i64,
            plugin.created_at,
        ],
    )?;
    Ok(plugin)
}

pub fn update(conn: &Connection, id: &str, input: &PluginInput) -> AppResult<Plugin> {
    let n = conn.execute(
        "UPDATE plugins SET name = ?2, kind = ?3, language_label = ?4, source = ?5, enabled = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            id,
            input.name,
            kind_to_str(input.kind),
            input.language_label,
            input.source,
            input.enabled as i64,
            now_millis(),
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("plugin {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM plugins WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("plugin {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_with_workspace() -> (Connection, String) {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        (conn, ws.id)
    }

    fn sample_input(name: &str) -> PluginInput {
        PluginInput {
            name: name.to_string(),
            kind: PluginKind::Codegen,
            language_label: "Python".to_string(),
            source: "module.exports = () => '';".to_string(),
            enabled: true,
        }
    }

    #[test]
    fn create_then_get_round_trips() {
        let (conn, ws) = mem_with_workspace();
        let created = create(&conn, &ws, &sample_input("My Codegen")).unwrap();
        assert_eq!(created.workspace_id, ws);
        assert_eq!(created.kind, PluginKind::Codegen);
        assert!(created.enabled);

        let fetched = get(&conn, &created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, "My Codegen");
        assert_eq!(fetched.language_label, "Python");
        assert_eq!(fetched.source, created.source);
    }

    #[test]
    fn list_by_workspace_excludes_other_workspaces() {
        let (conn, ws1) = mem_with_workspace();
        create(&conn, &ws1, &sample_input("Plugin A")).unwrap();

        let ws2 = crate::store::workspaces::create(&conn, "Other Workspace").unwrap();
        create(&conn, &ws2.id, &sample_input("Plugin B")).unwrap();

        let ws1_plugins = list_by_workspace(&conn, &ws1, None).unwrap();
        assert_eq!(ws1_plugins.len(), 1);
        assert_eq!(ws1_plugins[0].name, "Plugin A");

        let ws2_plugins = list_by_workspace(&conn, &ws2.id, None).unwrap();
        assert_eq!(ws2_plugins.len(), 1);
        assert_eq!(ws2_plugins[0].name, "Plugin B");
    }

    #[test]
    fn list_by_workspace_filters_by_kind() {
        let (conn, ws) = mem_with_workspace();
        create(&conn, &ws, &sample_input("Codegen Plugin")).unwrap();
        let mut import_input = sample_input("Import Plugin");
        import_input.kind = PluginKind::Import;
        create(&conn, &ws, &import_input).unwrap();
        let mut export_input = sample_input("Export Plugin");
        export_input.kind = PluginKind::Export;
        create(&conn, &ws, &export_input).unwrap();

        let codegen = list_by_workspace(&conn, &ws, Some(PluginKind::Codegen)).unwrap();
        assert_eq!(codegen.len(), 1);
        assert_eq!(codegen[0].name, "Codegen Plugin");

        let import = list_by_workspace(&conn, &ws, Some(PluginKind::Import)).unwrap();
        assert_eq!(import.len(), 1);
        assert_eq!(import[0].name, "Import Plugin");

        let export = list_by_workspace(&conn, &ws, Some(PluginKind::Export)).unwrap();
        assert_eq!(export.len(), 1);
        assert_eq!(export[0].name, "Export Plugin");

        let all = list_by_workspace(&conn, &ws, None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn update_changes_fields() {
        let (conn, ws) = mem_with_workspace();
        let created = create(&conn, &ws, &sample_input("Original")).unwrap();

        let updated_input = PluginInput {
            name: "Renamed".to_string(),
            kind: PluginKind::Export,
            language_label: "Insomnia v4".to_string(),
            source: "module.exports = () => 'updated';".to_string(),
            enabled: false,
        };
        let updated = update(&conn, &created.id, &updated_input).unwrap();
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.kind, PluginKind::Export);
        assert_eq!(updated.language_label, "Insomnia v4");
        assert_eq!(updated.source, "module.exports = () => 'updated';");
        assert!(!updated.enabled);
        assert!(updated.updated_at >= created.updated_at);

        let refetched = get(&conn, &created.id).unwrap();
        assert_eq!(refetched.name, "Renamed");
    }

    #[test]
    fn update_missing_plugin_errors_not_found() {
        let (conn, _ws) = mem_with_workspace();
        let result = update(&conn, "no-such-id", &sample_input("X"));
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_removes_the_row() {
        let (conn, ws) = mem_with_workspace();
        let created = create(&conn, &ws, &sample_input("Doomed")).unwrap();
        delete(&conn, &created.id).unwrap();

        let result = get(&conn, &created.id);
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_missing_plugin_errors_not_found() {
        let (conn, _ws) = mem_with_workspace();
        let result = delete(&conn, "no-such-id");
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn list_ordering_is_stable_by_name() {
        let (conn, ws) = mem_with_workspace();
        create(&conn, &ws, &sample_input("Zebra")).unwrap();
        create(&conn, &ws, &sample_input("Apple")).unwrap();
        create(&conn, &ws, &sample_input("Mango")).unwrap();

        let plugins = list_by_workspace(&conn, &ws, None).unwrap();
        let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Apple", "Mango", "Zebra"]);
    }
}
