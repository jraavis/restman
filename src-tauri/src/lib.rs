mod auth;
mod backup;
mod codegen;
mod commands;
mod engine;
mod error;
mod interop;
mod model;
pub mod plugins;
mod scripting;
mod secrets;
mod store;
mod sync;
mod util;
mod vars;
mod workspace;

use std::sync::{Arc, Mutex};
use store::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Auto-update (Phase 8). `plugins.updater.pubkey` in tauri.conf.json
        // is a real generated keypair (private half deliberately kept out of
        // git — see src-tauri/.gitignore), not a placeholder: registering
        // with an empty/malformed key risks failing at startup, which would
        // break this repo's own `cargo tauri dev` boot-verification gate.
        // The endpoint is only ever hit when the frontend calls `check()`
        // (see `src/features/settings/SettingsDialog.tsx`'s About tab), so
        // it staying unpublished doesn't affect startup either.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // App-local data dir holds the single SQLite database.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("restman.db");

            let mut conn = store::db::open(&db_path)?;
            store::workspaces::ensure_default(&mut conn)?;
            store::variables::migrate_plaintext_secrets_to_keychain(&conn);

            let cookie_jar = Arc::new(reqwest_cookie_store::CookieStoreMutex::new(
                reqwest_cookie_store::CookieStore::new(),
            ));

            app.manage(AppState {
                db: Mutex::new(conn),
                cookie_jar,
                streams: Arc::new(Mutex::new(std::collections::HashMap::new())),
                mock_servers: Mutex::new(std::collections::HashMap::new()),
                grpc_schema_cache: Mutex::new(std::collections::HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::send_request,
            commands::introspect_graphql_schema,
            commands::start_oauth2_authorization,
            commands::get_oauth2_status,
            commands::list_workspaces,
            commands::active_workspace,
            commands::create_workspace,
            commands::update_workspace,
            commands::delete_workspace,
            commands::set_active_workspace,
            commands::get_workspace_settings,
            commands::set_workspace_settings,
            commands::list_collections,
            commands::create_collection,
            commands::update_collection,
            commands::update_collection_auth,
            commands::delete_collection,
            commands::move_collection,
            commands::reorder_collections,
            commands::duplicate_collection,
            commands::list_requests,
            commands::get_request,
            commands::create_request,
            commands::update_request,
            commands::delete_request,
            commands::move_request,
            commands::reorder_requests,
            commands::duplicate_request,
            commands::set_request_tags,
            commands::search_requests,
            commands::list_tags,
            commands::create_tag,
            commands::update_tag,
            commands::delete_tag,
            commands::list_environments,
            commands::create_environment,
            commands::update_environment,
            commands::delete_environment,
            commands::set_active_environment,
            commands::active_environment,
            commands::list_variables,
            commands::create_variable,
            commands::update_variable,
            commands::delete_variable,
            commands::get_secret_backend_status,
            commands::list_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::replay_history_entry,
            commands::get_history_retention,
            commands::set_history_retention,
            commands::list_tabs,
            commands::create_tab,
            commands::update_tab_draft,
            commands::set_tab_request_id,
            commands::set_active_tab,
            commands::reorder_tabs,
            commands::close_tab,
            commands::close_other_tabs,
            commands::close_all_tabs,
            commands::clear_cookies,
            commands::list_cookies,
            commands::delete_cookie,
            commands::sse_connect,
            commands::ws_connect,
            commands::ws_send,
            commands::grpc_discover_schema,
            commands::grpc_connect,
            commands::grpc_send,
            commands::grpc_finish_sending,
            commands::stream_disconnect,
            commands::run_collection_tests,
            commands::get_oauth_token_preview,
            commands::preview_import,
            commands::preview_import_bruno_directory,
            commands::apply_collection_import,
            commands::export_collection,
            commands::preview_environment_import,
            commands::apply_environment_import,
            commands::export_environment,
            commands::export_restman,
            commands::preview_restman_import,
            commands::apply_restman_import,
            commands::generate_code,
            commands::write_file_bytes,
            commands::list_plugins,
            commands::create_plugin,
            commands::update_plugin,
            commands::delete_plugin,
            commands::export_plugin,
            commands::import_plugin,
            commands::preview_plugin_codegen,
            commands::preview_plugin_import,
            commands::preview_plugin_export,
            commands::list_mock_servers,
            commands::create_mock_server,
            commands::create_mock_server_from_collection,
            commands::update_mock_server,
            commands::delete_mock_server,
            commands::list_mock_rules,
            commands::create_mock_rule,
            commands::update_mock_rule,
            commands::delete_mock_rule,
            commands::start_mock_server,
            commands::stop_mock_server,
            commands::export_mock_server,
            commands::import_mock_server,
            commands::list_running_mock_server_ids,
            commands::sync_export,
            commands::sync_import,
            commands::create_backup,
            commands::restore_backup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
