//! SSE (`text/event-stream`) transport and framing.
//!
//! `open()` does the network part and can't be exercised in this sandbox (no
//! outbound network from `cargo test` here — see project notes); it's kept
//! thin so it's correct by inspection. `FrameParser` is the part with actual
//! logic (buffering, field parsing, dispatch-on-blank-line) and is fully
//! unit-tested below without touching the network.
//!
//! Deliberately not full WHATWG `EventSource` semantics: no `retry:`
//! handling and no auto-reconnect using a last-event-id. This is a
//! point-in-time inspection tool, not a browser engine — the user reconnects
//! manually if a stream drops.

use crate::error::AppError;
use crate::model::http::HeaderEntry;
use crate::model::streaming::SseEvent;
use reqwest::{Client, Response};

/// Opens the SSE connection and validates the response status. The caller
/// drives the actual read loop via `Response::chunk()`.
pub async fn open(client: &Client, url: &str, headers: &[HeaderEntry]) -> Result<Response, AppError> {
    let mut builder = client.get(url).header("Accept", "text/event-stream");
    for h in headers.iter().filter(|h| h.enabled && !h.name.trim().is_empty()) {
        builder = builder.header(&h.name, &h.value);
    }
    let resp = builder.send().await?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "server returned {}",
            resp.status()
        )));
    }
    Ok(resp)
}

/// Drains complete `\n`-terminated lines from `buf`, leaving any trailing
/// partial line buffered for the next chunk. Tolerates a trailing `\r`
/// (CRLF) per the SSE spec; decodes lossily since a multi-byte UTF-8
/// character could in principle straddle a chunk boundary elsewhere in
/// `buf`, but never straddles a line we've already isolated here.
pub fn drain_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    lines
}

/// Incremental SSE frame parser: feed it lines (no trailing CR/LF), get back
/// a dispatched `Message` whenever a blank line terminates a non-empty frame.
#[derive(Default)]
pub struct FrameParser {
    event: Option<String>,
    data: Vec<String>,
    id: Option<String>,
}

impl FrameParser {
    pub fn feed_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None; // comment line, per spec
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            "id" => self.id = Some(value.to_string()),
            // "retry" and anything unknown: no auto-reconnect in this tool.
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.event.is_none() && self.data.is_empty() && self.id.is_none() {
            return None; // blank line between frames; nothing buffered
        }
        let event = SseEvent::Message {
            event: self.event.take(),
            data: self.data.join("\n"),
            id: self.id.take(),
        };
        self.data.clear();
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(parser: &mut FrameParser, lines: &[&str]) -> Vec<SseEvent> {
        lines.iter().filter_map(|l| parser.feed_line(l)).collect()
    }

    #[test]
    fn dispatches_simple_data_only_frame() {
        let mut p = FrameParser::default();
        let events = feed_all(&mut p, &["data: hello", ""]);
        match &events[..] {
            [SseEvent::Message { event, data, id }] => {
                assert_eq!(event, &None);
                assert_eq!(data, "hello");
                assert_eq!(id, &None);
            }
            other => panic!("expected one Message event, got {other:?}"),
        }
    }

    #[test]
    fn joins_multiple_data_lines_with_newline() {
        let mut p = FrameParser::default();
        let events = feed_all(&mut p, &["data: line one", "data: line two", ""]);
        match &events[..] {
            [SseEvent::Message { data, .. }] => assert_eq!(data, "line one\nline two"),
            other => panic!("expected one Message event, got {other:?}"),
        }
    }

    #[test]
    fn captures_event_and_id_fields() {
        let mut p = FrameParser::default();
        let events = feed_all(&mut p, &["event: ping", "id: 42", "data: payload", ""]);
        match &events[..] {
            [SseEvent::Message { event, data, id }] => {
                assert_eq!(event.as_deref(), Some("ping"));
                assert_eq!(data, "payload");
                assert_eq!(id.as_deref(), Some("42"));
            }
            other => panic!("expected one Message event, got {other:?}"),
        }
    }

    #[test]
    fn ignores_comment_lines() {
        let mut p = FrameParser::default();
        let events = feed_all(&mut p, &[": keep-alive", "data: real", ""]);
        match &events[..] {
            [SseEvent::Message { data, .. }] => assert_eq!(data, "real"),
            other => panic!("expected one Message event, got {other:?}"),
        }
    }

    #[test]
    fn blank_line_with_nothing_buffered_dispatches_nothing() {
        let mut p = FrameParser::default();
        let events = feed_all(&mut p, &["", "", ""]);
        assert!(events.is_empty());
    }

    #[test]
    fn resets_event_and_id_but_not_unrelated_state_between_frames() {
        let mut p = FrameParser::default();
        let first = feed_all(&mut p, &["event: a", "data: 1", ""]);
        let second = feed_all(&mut p, &["data: 2", ""]);
        assert!(matches!(
            &first[..],
            [SseEvent::Message { event: Some(e), .. }] if e == "a"
        ));
        // second frame didn't repeat "event: a" — should come back empty, not stale.
        assert!(matches!(
            &second[..],
            [SseEvent::Message { event: None, data, .. }] if data == "2"
        ));
    }

    #[test]
    fn field_with_no_colon_is_treated_as_field_name_with_empty_value() {
        let mut p = FrameParser::default();
        // A bare "data" line (no colon) is valid per spec: empty-string data.
        let events = feed_all(&mut p, &["data", ""]);
        match &events[..] {
            [SseEvent::Message { data, .. }] => assert_eq!(data, ""),
            other => panic!("expected one Message event, got {other:?}"),
        }
    }

    #[test]
    fn drain_lines_splits_on_lf_and_buffers_partial_tail() {
        let mut buf = b"data: a\ndata: b\nincomple".to_vec();
        let lines = drain_lines(&mut buf);
        assert_eq!(lines, vec!["data: a", "data: b"]);
        assert_eq!(buf, b"incomple");
    }

    #[test]
    fn drain_lines_strips_trailing_cr() {
        let mut buf = b"data: a\r\n".to_vec();
        let lines = drain_lines(&mut buf);
        assert_eq!(lines, vec!["data: a"]);
        assert!(buf.is_empty());
    }
}
