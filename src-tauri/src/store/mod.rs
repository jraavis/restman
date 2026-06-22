//! Persistence layer: SQLite connection, schema migrations, and entity repos.

pub mod collections;
pub mod db;
pub mod environments;
pub mod history;
pub mod migrations;
pub mod requests;
pub mod tabs;
pub mod tags;
pub mod variables;
pub mod workspaces;

use reqwest_cookie_store::CookieStoreMutex;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Managed Tauri state holding the single SQLite connection and the
/// shared cookie jar for cookie-based session auth.
///
/// IMPORTANT (async commands): the std `MutexGuard` is `!Send`. In an `async`
/// command, take the lock, do the DB work, and drop the guard *before* any
/// `.await`, otherwise the command future becomes `!Send` and Tauri rejects it
/// at compile time. If DB work ever gets heavy, move it into `spawn_blocking`
/// rather than switching to an async mutex.
pub struct AppState {
    pub db: Mutex<Connection>,
    /// Shared RFC 6265 cookie jar. Requests with `send_cookies: true` share
    /// this store — cookies set by one request are replayed on subsequent ones.
    pub cookie_jar: Arc<CookieStoreMutex>,
}
