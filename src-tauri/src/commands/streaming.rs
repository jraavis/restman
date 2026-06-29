//! Tauri commands for long-lived protocol connections. Thin glue: builds a
//! client via the same transport (proxy/cert/default-header) config HTTP
//! requests use, spawns a task that drives `engine::sse`/`engine::ws` and
//! forwards events to the frontend over a `Channel`, and tracks the task so
//! `stream_disconnect` can stop it early. WebSocket connects *through* that
//! reqwest client (see `engine::ws`) so it inherits the same transport
//! settings SSE/HTTP do — it does not use tokio-tungstenite's own
//! `connect_async`. See `engine::{sse,ws}` for the framing logic and tests;
//! this module's only logic is the send-path error handling in `ws_send`.
//!
//! ## gRPC (17d-8 / #29)
//!
//! `grpc_connect` follows the same connect-then-spawn-then-register shape as
//! `sse_connect`/`ws_connect`, with one structural difference: building the
//! `DescriptorPool` (from inline `.proto` source — see `GrpcConnectArgs`'
//! doc comment for why schema discovery isn't wired in yet) happens
//! synchronously in the command, so a bad `.proto` or an unknown method
//! returns a command error immediately, the same way `build_ws_client`'s
//! transport errors do — never silently inside the spawned task where the
//! caller has nothing left to catch it. The network connect and the actual
//! RPC drive *do* happen inside the spawned task, same as `run_ws`, so a
//! connect failure becomes a `GrpcEvent::Error` on the channel rather than a
//! command error (there's no command-result caller left listening by the
//! time a long-lived connection actually drops).
//!
//! Known limitation, surfaced here and in `GrpcTransport::connect`'s doc
//! comment: gRPC connections do not yet honor the workspace's proxy or
//! client-certificate settings (the hand-rolled h2/rustls client has no path
//! to either — see that doc comment for the full explanation). A configured
//! proxy/client-cert is a clean `AppError` from `grpc_connect`, never a
//! silent direct/unauthenticated connection.

use crate::engine::grpc::{self, transport::GrpcTransport};
use crate::engine::http::{build_client, build_ws_client};
use crate::engine::{sse, ws};
use crate::error::{AppError, AppResult};
use crate::model::grpc::{GrpcConnectArgs, GrpcEvent, GrpcOutbound};
use crate::model::http::{HeaderEntry, RequestOptions};
use crate::model::streaming::{message_to_event, outbound_to_message, SseEvent, WsEvent, WsOutbound};
use crate::store::{AppState, StreamHandle, StreamSender};
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
            sender: Some(StreamSender::Ws(tx)),
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
    match handle.sender.as_ref() {
        Some(StreamSender::Ws(sender)) => sender
            .send(msg)
            .map_err(|_| AppError::Other("connection closed".into())),
        Some(StreamSender::Grpc(_)) => Err(AppError::Other(
            "this is a gRPC connection; use grpc_send instead of ws_send".into(),
        )),
        None => Err(AppError::Other("connection does not support sending".into())),
    }
}

/// Opens a gRPC connection and drives the first request: compiles the
/// `DescriptorPool` and resolves the method *synchronously*, here in the
/// command (a bad `.proto` or unknown method is a command error the caller
/// can act on immediately — never silently inside the spawned task), then
/// connects and drives the actual RPC inside a spawned task that forwards
/// `GrpcEvent`s over `channel`, the same connect-then-spawn-then-register
/// shape as `sse_connect`/`ws_connect`.
///
/// The streaming mode isn't a command argument — once the method descriptor
/// is resolved, `is_client_streaming()`/`is_server_streaming()` give it
/// directly, so unary uses `call_unary` and the other three modes use
/// `drive_streaming_call` (see `engine::grpc`'s module docs for why those are
/// two separate drive functions rather than one). Only client-streaming/bidi
/// register a `StreamSender::Grpc` (so `grpc_send`/`grpc_finish_sending` have
/// something to send on); unary and server-streaming have nothing to send
/// after the initial request, so their entries get `sender: None`, same as
/// SSE.
#[tauri::command]
pub async fn grpc_connect(
    state: State<'_, AppState>,
    channel: Channel<GrpcEvent>,
    workspace_id: String,
    args: GrpcConnectArgs,
) -> AppResult<String> {
    let transport_overrides = {
        let conn = state.db.lock().unwrap();
        crate::workspace::resolve_transport(&conn, &workspace_id)?
    };

    let pool =
        grpc::schema::compile_proto_set(&args.proto_files, std::slice::from_ref(&args.entry_point))?;
    let method = grpc::resolve_method(&pool, &args.method_full_name)?;
    let is_unary = !method.is_client_streaming() && !method.is_server_streaming();

    let connection_id = uuid::Uuid::new_v4().to_string();
    let streams = std::sync::Arc::clone(&state.streams);
    let task_id = connection_id.clone();

    let GrpcConnectArgs {
        url,
        method_full_name,
        request,
        ..
    } = args;

    let (sender, handle) = if is_unary {
        let handle = tokio::spawn(async move {
            run_grpc_unary(url, transport_overrides, pool, method_full_name, request, &channel)
                .await;
            streams.lock().unwrap().remove(&task_id);
        });
        (None, handle)
    } else {
        // Client-streaming/server-streaming/bidi all funnel through the same
        // `drive_streaming_call`. The initial request is pre-queued onto the
        // very channel `run_grpc_streaming` forwards to it — an unbounded
        // `send` succeeds with no receiver polling yet, so it's simply first
        // in line once the drive loop starts reading. Only client-streaming
        // keeps the sender alive afterwards (for `grpc_send`/
        // `grpc_finish_sending`); server-streaming drops it immediately so
        // the request side half-closes right after that one message, the
        // same shape 17d-7's server-streaming loopback test exercises.
        let (tx, rx) = mpsc::unbounded_channel::<grpc::GrpcRequestMsg>();
        let _ = tx.send(Some(request));
        let sender = if method.is_client_streaming() {
            Some(StreamSender::Grpc(tx))
        } else {
            drop(tx);
            None
        };
        let handle = tokio::spawn(async move {
            run_grpc_streaming(url, transport_overrides, pool, method_full_name, rx, &channel)
                .await;
            streams.lock().unwrap().remove(&task_id);
        });
        (sender, handle)
    };

    state.streams.lock().unwrap().insert(
        connection_id.clone(),
        StreamHandle { task: handle, sender },
    );

    Ok(connection_id)
}

/// Sends another request message on a live client-streaming/bidi gRPC
/// connection. Mirrors `ws_send`'s shape exactly: synchronous, returns
/// errors immediately rather than swallowing them in the drive loop.
#[tauri::command]
pub fn grpc_send(
    state: State<'_, AppState>,
    connection_id: String,
    message: GrpcOutbound,
) -> AppResult<()> {
    let streams = state.streams.lock().unwrap();
    let handle = streams
        .get(&connection_id)
        .ok_or_else(|| AppError::Other("not connected".into()))?;
    match handle.sender.as_ref() {
        Some(StreamSender::Grpc(sender)) => sender
            .send(Some(message.request))
            .map_err(|_| AppError::Other("connection closed".into())),
        Some(StreamSender::Ws(_)) => Err(AppError::Other(
            "this is a WebSocket connection; use ws_send instead of grpc_send".into(),
        )),
        None => Err(AppError::Other(
            "connection does not support sending (unary and server-streaming gRPC calls only send their initial request)".into(),
        )),
    }
}

/// Half-closes the request side of a live client-streaming/bidi gRPC
/// connection: no more request messages will be sent, but the connection
/// stays open to receive the response(s) and final status. Distinct from
/// `stream_disconnect`, which aborts the task outright and would lose a
/// client-streaming call's only response (the server doesn't reply until the
/// request side half-closes — see `engine::grpc::drive_streaming_call`'s doc
/// comment) — this sends a graceful `None` on the same channel `grpc_send`
/// uses, which `drive_streaming_call` already maps to
/// `GrpcStream::half_close` (17d-7). Not part of `grpcMessageIpc.ts`'s mock
/// surface; added here because client-streaming/bidi cannot function without
/// some way to signal "done sending" short of tearing the connection down.
#[tauri::command]
pub fn grpc_finish_sending(state: State<'_, AppState>, connection_id: String) -> AppResult<()> {
    let streams = state.streams.lock().unwrap();
    let handle = streams
        .get(&connection_id)
        .ok_or_else(|| AppError::Other("not connected".into()))?;
    match handle.sender.as_ref() {
        Some(StreamSender::Grpc(sender)) => sender
            .send(None)
            .map_err(|_| AppError::Other("connection closed".into())),
        Some(StreamSender::Ws(_)) => Err(AppError::Other(
            "this is a WebSocket connection, which has no half-close concept".into(),
        )),
        None => Err(AppError::Other(
            "connection does not support sending, so there is nothing to finish".into(),
        )),
    }
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

/// Drives a unary gRPC call to completion: connects, then bridges
/// `engine::grpc::call_unary`'s one-shot `UnaryCallResult` onto the same
/// `GrpcEvent` sequence the streaming drive loop produces (`Open` ->
/// `Response?` -> `Status` -> `Closed`), so the frontend's `Channel<GrpcEvent>`
/// listener doesn't need to know which drive function actually ran.
/// `call_unary` itself doesn't speak `GrpcEvent` at all (it predates it,
/// landing in 17d-6 as a plain `Result`-returning function, and stays that
/// way — routing unary through `drive_streaming_call` instead would
/// half-close with an extra zero-length END_STREAM frame after the request
/// rather than `END_STREAM` on the request frame itself, a real behavioral
/// deviation from how a unary call is supposed to look on the wire).
async fn run_grpc_unary(
    url: String,
    transport_overrides: Option<crate::engine::http::TransportOverrides>,
    pool: prost_reflect::DescriptorPool,
    method_full_name: String,
    request: serde_json::Value,
    channel: &Channel<GrpcEvent>,
) {
    let mut transport = match GrpcTransport::connect(&url, transport_overrides.as_ref()).await {
        Ok(t) => t,
        Err(e) => {
            let _ = channel.send(GrpcEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    if channel.send(GrpcEvent::Open).is_err() {
        return;
    }

    match grpc::call_unary(&pool, &method_full_name, request, &mut transport).await {
        Ok(result) => {
            if let Some(message) = result.response {
                if channel.send(GrpcEvent::Response { message }).is_err() {
                    return;
                }
            }
            if channel
                .send(GrpcEvent::Status {
                    code: result.status.code,
                    message: result.status.message,
                })
                .is_err()
            {
                return;
            }
            let _ = channel.send(GrpcEvent::Closed);
        }
        Err(e) => {
            let _ = channel.send(GrpcEvent::Error {
                message: e.to_string(),
            });
        }
    }
}

/// Drives a client-streaming/server-streaming/bidi gRPC call to completion:
/// connects, then hands off to `engine::grpc::drive_streaming_call` (17d-7)
/// over a small relay mpsc channel that forwards its plain `GrpcEvent`s onto
/// the real `Channel<GrpcEvent>` — `drive_streaming_call` is intentionally
/// Tauri-agnostic (see its own doc comment for why: so it stays usable from
/// an offline test), so this is the one place that connects the two. The
/// relay can't be `tokio::spawn`ed (it would need to outlive `drive`, which
/// borrows `pool`/`transport` by reference and isn't `'static`); `tokio::
/// join!` runs both concurrently in this same task instead, which is exactly
/// what's needed since `drive` only progresses once something is actually
/// listening on the events channel it sends into.
///
/// `requests` already has the caller's initial request message pre-queued by
/// `grpc_connect` (an unbounded `send` succeeds before anything has polled
/// the receiver, so it's simply first in line) — no separate splice step is
/// needed here.
async fn run_grpc_streaming(
    url: String,
    transport_overrides: Option<crate::engine::http::TransportOverrides>,
    pool: prost_reflect::DescriptorPool,
    method_full_name: String,
    requests: mpsc::UnboundedReceiver<grpc::GrpcRequestMsg>,
    channel: &Channel<GrpcEvent>,
) {
    let mut transport = match GrpcTransport::connect(&url, transport_overrides.as_ref()).await {
        Ok(t) => t,
        Err(e) => {
            let _ = channel.send(GrpcEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };

    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<GrpcEvent>();
    let drive = grpc::drive_streaming_call(&pool, &method_full_name, &mut transport, requests, events_tx);
    let relay = async {
        while let Some(event) = events_rx.recv().await {
            if channel.send(event).is_err() {
                return;
            }
        }
    };
    tokio::join!(drive, relay);
}
