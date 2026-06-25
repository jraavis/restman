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

/// Events pushed to the frontend for a live WebSocket connection (#17b).
/// Binary frames travel as base64 in `data` (the IPC channel is JSON); the
/// `binary` flag tells the UI whether to decode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsEvent {
    /// Upgrade handshake succeeded; the socket is open for read/write.
    Open,
    /// One received data frame. `data` is the text verbatim, or base64 for a
    /// binary frame (`binary: true`).
    Message { binary: bool, data: String },
    /// The peer closed the connection. Terminal. `code`/`reason` come from the
    /// WS close frame when the peer supplied one.
    Closed {
        code: Option<u16>,
        reason: Option<String>,
    },
    /// The connection failed (handshake or mid-stream). Terminal.
    Error { message: String },
}

/// An outbound WS frame from the UI, deserialized from the `ws_send` arg.
/// `data` is text verbatim, or base64 to be decoded into a binary frame.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WsOutbound {
    pub binary: bool,
    pub data: String,
}

// --- Pure converters between tungstenite frames and our IPC types ---------
//
// These are the only logic worth unit-testing in the WS path (the network
// drive loop can't run in this sandbox, same as SSE). They live here so the
// command/engine layers stay thin.

use crate::error::AppError;
use base64::Engine as _;
use tokio_tungstenite::tungstenite::Message;

/// Maps a received tungstenite `Message` to a `WsEvent`. Returns `None` for
/// frames the transcript deliberately suppresses (Ping/Pong — tungstenite
/// answers Ping automatically on the next write; raw `Frame` never surfaces
/// on the read path).
pub fn message_to_event(msg: Message) -> Option<WsEvent> {
    match msg {
        Message::Text(s) => Some(WsEvent::Message {
            binary: false,
            data: s,
        }),
        Message::Binary(b) => Some(WsEvent::Message {
            binary: true,
            data: base64::engine::general_purpose::STANDARD.encode(&b),
        }),
        Message::Close(frame) => Some(WsEvent::Closed {
            code: frame.as_ref().map(|f| u16::from(f.code)),
            reason: frame
                .map(|f| f.reason.into_owned())
                .filter(|r| !r.is_empty()),
        }),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => None,
    }
}

/// Builds a tungstenite `Message` from an outbound UI frame, base64-decoding
/// the payload for binary frames. A malformed base64 binary payload is a
/// user/client error, surfaced as `AppError` rather than silently sent empty.
pub fn outbound_to_message(out: WsOutbound) -> Result<Message, AppError> {
    if out.binary {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(out.data.as_bytes())
            .map_err(|e| AppError::Other(format!("invalid base64 in binary frame: {e}")))?;
        Ok(Message::Binary(bytes))
    } else {
        Ok(Message::Text(out.data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::frame::{coding::CloseCode, CloseFrame};

    #[test]
    fn text_frame_maps_to_text_message_event() {
        let ev = message_to_event(Message::Text("hello".into())).unwrap();
        assert_eq!(
            ev,
            WsEvent::Message {
                binary: false,
                data: "hello".into()
            }
        );
    }

    #[test]
    fn binary_frame_is_base64_encoded() {
        let ev = message_to_event(Message::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF])).unwrap();
        assert_eq!(
            ev,
            WsEvent::Message {
                binary: true,
                data: "3q2+7w==".into()
            }
        );
    }

    #[test]
    fn close_frame_carries_code_and_reason() {
        let frame = Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "bye".into(),
        }));
        assert_eq!(
            message_to_event(frame).unwrap(),
            WsEvent::Closed {
                code: Some(1000),
                reason: Some("bye".into())
            }
        );
    }

    #[test]
    fn bare_close_has_no_code_or_reason() {
        assert_eq!(
            message_to_event(Message::Close(None)).unwrap(),
            WsEvent::Closed {
                code: None,
                reason: None
            }
        );
    }

    #[test]
    fn empty_close_reason_is_dropped() {
        let frame = Message::Close(Some(CloseFrame {
            code: CloseCode::Away,
            reason: "".into(),
        }));
        assert_eq!(
            message_to_event(frame).unwrap(),
            WsEvent::Closed {
                code: Some(1001),
                reason: None
            }
        );
    }

    #[test]
    fn ping_and_pong_are_suppressed() {
        assert!(message_to_event(Message::Ping(vec![1])).is_none());
        assert!(message_to_event(Message::Pong(vec![1])).is_none());
    }

    #[test]
    fn outbound_text_round_trips() {
        let msg = outbound_to_message(WsOutbound {
            binary: false,
            data: "ping".into(),
        })
        .unwrap();
        assert!(matches!(msg, Message::Text(s) if s == "ping"));
    }

    #[test]
    fn outbound_binary_decodes_base64() {
        let msg = outbound_to_message(WsOutbound {
            binary: true,
            data: "3q2+7w==".into(),
        })
        .unwrap();
        assert!(matches!(msg, Message::Binary(b) if b == vec![0xDE, 0xAD, 0xBE, 0xEF]));
    }

    #[test]
    fn outbound_binary_rejects_bad_base64() {
        let err = outbound_to_message(WsOutbound {
            binary: true,
            data: "not!base64".into(),
        });
        assert!(err.is_err());
    }
}
