//! Event types pushed to the frontend over a Tauri `Channel` for long-lived
//! protocol connections (SSE now; WebSocket/gRPC in later sub-phases each
//! get their own event type rather than being forced into this one — an SSE
//! frame's `event`/`id` fields don't translate to a WS text/binary frame or
//! a gRPC message).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SseEvent {
    /// Connection established and the server responded with a success status.
    Open,
    /// One dispatched SSE frame (terminated by a blank line in the stream).
    Message {
        event: Option<String>,
        data: String,
        id: Option<String>,
    },
    /// The connection failed, or failed while streaming. Terminal — no more
    /// events follow on this channel.
    Error { message: String },
    /// The server closed the stream normally. Terminal.
    Closed,
}
