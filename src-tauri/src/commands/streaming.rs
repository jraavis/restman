//! Tauri commands for long-lived protocol connections. Thin glue: builds a
//! client via the same transport (proxy/cert) config HTTP requests use,
//! spawns a task that drives `engine::sse` and forwards parsed events to the
//! frontend over a `Channel`, and tracks the task so `*_disconnect` can stop
//! it early. See `engine::sse` for the actual framing logic and its tests —
//! this module has no independent logic to unit-test (and no network access
//! in this sandbox to exercise it with anyway).

use crate::engine::http::build_client;
use crate::engine::sse;
use crate::error::AppResult;
use crate::model::http::{HeaderEntry, RequestOptions};
use crate::model::streaming::SseEvent;
use crate::store::AppState;
use tauri::ipc::Channel;
use tauri::State;

/// SSE connections are long-lived by design, so we skip reqwest's
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
        run(client, &url, &headers, &channel).await;
        streams.lock().unwrap().remove(&task_id);
    });
    state.streams.lock().unwrap().insert(connection_id.clone(), handle);

    Ok(connection_id)
}

#[tauri::command]
pub fn sse_disconnect(state: State<'_, AppState>, connection_id: String) -> AppResult<()> {
    if let Some(handle) = state.streams.lock().unwrap().remove(&connection_id) {
        handle.abort();
    }
    Ok(())
}

/// Drives the connection to completion, sending `Open`/`Message`/`Error`/
/// `Closed` events as they occur. Never returns an error directly — failures
/// become an `Error` event on the channel, since there's no command-result
/// caller left listening by the time the stream ends.
async fn run(client: reqwest::Client, url: &str, headers: &[HeaderEntry], channel: &Channel<SseEvent>) {
    let mut resp = match sse::open(&client, url, headers).await {
        Ok(r) => r,
        Err(e) => {
            let _ = channel.send(SseEvent::Error { message: e.to_string() });
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
                let _ = channel.send(SseEvent::Error { message: e.to_string() });
                return;
            }
        }
    }
}
