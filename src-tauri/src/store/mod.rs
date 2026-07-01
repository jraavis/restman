//! Persistence layer: SQLite connection, schema migrations, and entity repos.

pub mod collections;
pub mod db;
pub mod environments;
pub mod history;
pub mod migrations;
pub mod mock_servers;
pub mod plugins;
pub mod requests;
pub mod tabs;
pub mod tags;
pub mod variables;
pub mod workspace_settings;
pub mod workspaces;

use reqwest_cookie_store::CookieStoreMutex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::engine::grpc::GrpcRequestMsg;

/// The outbound-send half of a live streaming connection, when the protocol
/// supports sending after connect at all. A plain `enum` rather than a
/// generic `StreamHandle<T>` because `AppState::streams` is one
/// `HashMap<String, StreamHandle>` shared across every protocol — a generic
/// handle would need the map itself to be generic (or boxed/`dyn`), which
/// buys nothing here since there are exactly two concrete payload shapes
/// (WS frames, gRPC request JSON) and `ws_send`/`grpc_send` each only ever
/// need to match their own arm and reject the other (mirroring how `ws_send`
/// already rejected `sender: None` before this enum existed).
pub enum StreamSender {
    Ws(UnboundedSender<WsMessage>),
    Grpc(UnboundedSender<GrpcRequestMsg>),
}

/// A live streaming connection's handle, keyed by connection id in
/// `AppState::streams`. Every protocol gets `task` (abort on disconnect);
/// only protocols that support sending after connect (WebSocket; gRPC
/// client-streaming/bidi) populate `sender`. SSE and unary/server-streaming
/// gRPC are send-once-then-receive-only, so their entries always have
/// `sender: None`.
pub struct StreamHandle {
    pub task: JoinHandle<()>,
    pub sender: Option<StreamSender>,
}

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
    /// Live streaming connections (SSE/WS/gRPC), keyed by a connection id
    /// generated at connect time. Each entry's task removes itself on natural
    /// completion; `stream_disconnect` removes-and-aborts explicitly.
    /// `Arc` so the spawned task can self-remove without borrowing `AppState`.
    pub streams: Arc<Mutex<HashMap<String, StreamHandle>>>,
}
