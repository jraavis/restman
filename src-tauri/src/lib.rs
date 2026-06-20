mod commands;
mod engine;
mod error;
mod model;
mod store;
mod util;

use std::sync::Mutex;
use store::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // App-local data dir holds the single SQLite database.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("restman.db");

            let mut conn = store::db::open(&db_path)?;
            store::workspaces::ensure_default(&mut conn)?;
            app.manage(AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::send_request,
            commands::list_workspaces,
            commands::active_workspace,
            commands::create_workspace,
            commands::set_active_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
