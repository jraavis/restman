//! Tauri commands for long-lived protocol connections. Thin glue: builds a
//! client via the same transport (proxy/cert/default-header) config HTTP
//! requests use, spawns a task that drives `engine::sse`/`engine::ws` and
//! forwards events to the frontend over a `Channel`, and tracks the task so
//! `stream_disconnect` can stop it early. WebSocket connects *through* that
//! reqwest client (see `engine::ws`) so it inherits the same transport
//! settings SSE/HTTP do — it does not use tokio-tungstenite's own
//! `connect_async`. See `engine::{sse,ws}` for the framing logic and tests;
//! this module's only logic is the send-path error handling in `ws_send`.

use crate::engine::http::{build_client, build_ws_client};
use crate::engine::{sse, ws};
use crate::error::{AppError, AppResult};
use crate::model::http::{HeaderEntry, RequestOptions};
use crate::model::streaming::{message_to_event, outbound_to_message, SseEvent, WsEvent, WsOutbound};
use crate::store::{AppState, StreamHandle};
use futures_util::{SinkExt, StreamExt};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Streaming connections are long-lived by design, so we skip reqwest's
/// total-request timeout entirely (the default is 30s, which would kill any
/// stream that outlives it) rather than reusing `RequestOptions::default()`.
fn streaming_options() -> RequestOptions {
    RequestOptions {
        timeout_secs: 60 * 60 * 24,
        ..RequestOptions::default()
    }
}

#[tauri::command]
pub async fn sse_connect(
    state: State<'_, AppState>,
    channel: Channel<SseEvent>,
    workspace_id: String,
    url: String,
    headers: Vec<HeaderEntry>,
) -> AppResult<String> {
    let transport = {
        let conn = state.db.lock().unwrap();
        crate::workspace::resolve_transport(&conn, &workspace_id)?
    };
    let client = build_client(&streaming_options(), None, transport.as_ref())?;

    let connection_id = uuid::Uuid::new_v4().to_string();
    let streams = std::sync::Arc::clone(&state.streams);
    let task_id = connection_id.clone();

    let handle = tokio::spawn(async move {
        run_sse(client, &url, &headers, &channel).await;
        streams.lock().unwrap().remove(&task_id);
    });
    state.streams.lock().unwrap().insert(
        connection_id.clone(),
        StreamHandle {
            task: handle,
            sender: None,
        },
    );

    Ok(connection_id)
}

#[tauri::command]
pub async fn ws_connect(
    state: State<'_, AppState>,
    channel: Channel<WsEvent>,
    workspace_id: String,
    url: String,
    headers: Vec<HeaderEntry>,
) -> AppResult<String> {
    let transport = {
        let conn = state.db.lock().unwrap();
        crate::workspace::resolve_transport(&conn, &workspace_id)?
    };
    let client = build_ws_client(&streaming_options(), transport.as_ref())?;

    let (tx, rx) = mpsc::unbounded_channel::<WsMessage>();
    let connection_id = uuid::Uuid::new_v4().to_string();
    let streams = std::sync::Arc::clone(&state.streams);
    let task_id = connection_id.clone();

    let handle = tokio::spawn(async move {
        run_ws(client, &url, &headers, rx, &channel).await;
        streams.lock().unwrap().remove(&task_id);
    });
    state.streams.lock().unwrap().insert(
        connection_id.clone(),
        StreamHandle {
            task: handle,
            sender: Some(tx),
        },
    );

    Ok(connection_id)
}

/// Sends a frame (text or binary) on a live WebSocket connection. Binary
/// payloads arrive base64-encoded; decoding happens here so malformed base64
/// is returned to the caller synchronously rather than swallowed in the drive
/// loop. Ephemeral-client scope (see PLAN.md #17b): no persistence.
#[tauri::command]
pub fn ws_send(
    state: State<'_, AppState>,
    connection_id: String,
    message: WsOutbound,
) -> AppResult<()> {
    let msg = outbound_to_message(message)?;
    let streams = state.streams.lock().unwrap();
    let handle = streams
        .get(&connection_id)
        .ok_or_else(|| AppError::Other("not connected".into()))?;
    let sender = handle
        .sender
        .as_ref()
        .ok_or_else(|| AppError::Other("connection does not support sending".into()))?;
    sender
        .send(msg)
        .map_err(|_| AppError::Other("connection closed".into()))
}

/// Stops a live connection of any protocol (SSE/WS/gRPC) and drops its entry.
/// Disconnect never needed protocol-specific knowledge — it only aborts the
/// task (and thereby drops the outbound sender, closing the socket) — so one
/// command serves all of them rather than a near-duplicate `*_disconnect` per
/// protocol.
#[tauri::command]
pub fn stream_disconnect(state: State<'_, AppState>, connection_id: String) -> AppResult<()> {
    if let Some(handle) = state.streams.lock().unwrap().remove(&connection_id) {
        handle.task.abort();
    }
    Ok(())
}

/// Drives an SSE connection to completion, sending `Open`/`Message`/`Error`/
/// `Closed` events as they occur. Never returns an error directly — failures
/// become an `Error` event on the channel, since there's no command-result
/// caller left listening by the time the stream ends.
async fn run_sse(
    client: reqwest::Client,
    url: &str,
    headers: &[HeaderEntry],
    channel: &Channel<SseEvent>,
) {
    let mut resp = match sse::open(&client, url, headers).await {
        Ok(r) => r,
        Err(e) => {
            let _ = channel.send(SseEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    if channel.send(SseEvent::Open).is_err() {
        return;
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut parser = sse::FrameParser::default();
    loop {
        match resp.chunk().await {
            Ok(Some(bytes)) => {
                buf.extend_from_slice(&bytes);
                for line in sse::drain_lines(&mut buf) {
                    if let Some(event) = parser.feed_line(&line) {
                        if channel.send(event).is_err() {
                            return;
                        }
                    }
                }
            }
            Ok(None) => {
                let _ = channel.send(SseEvent::Closed);
                return;
            }
            Err(e) => {
                let _ = channel.send(SseEvent::Error {
                    message: e.to_string(),
                });
                return;
            }
        }
    }
}

/// Drives a WebSocket connection to completion: forwards received frames as
/// `WsEvent`s (Ping/Pong suppressed — tungstenite auto-replies to Ping on the
/// next outgoing write, which the `rx` arm of this loop keeps live) and writes
/// any frame handed in over `rx` (from `ws_send`). The handshake runs through
/// the reqwest client, so proxy/client-cert/default-header transport settings
/// apply (see `engine::ws`).
async fn run_ws(
    client: reqwest::Client,
    url: &str,
    headers: &[HeaderEntry],
    mut rx: mpsc::UnboundedReceiver<WsMessage>,
    channel: &Channel<WsEvent>,
) {
    let mut ws = match ws::connect(&client, url, headers).await {
        Ok(ws) => ws,
        Err(e) => {
            let _ = channel.send(WsEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    if channel.send(WsEvent::Open).is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = ws.next() => {
                match incoming {
                    Some(Ok(msg)) => {
                        if let Some(event) = message_to_event(msg) {
                            let terminal = matches!(event, WsEvent::Closed { .. });
                            if channel.send(event).is_err() || terminal {
                                return;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let _ = channel.send(WsEvent::Error { message: e.to_string() });
                        return;
                    }
                    None => {
                        let _ = channel.send(WsEvent::Closed { code: None, reason: None });
                        return;
                    }
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Some(msg) => {
                        if ws.send(msg).await.is_err() {
                            return;
                        }
                    }
                    None => return,
                }
            }
        }
    }
}
