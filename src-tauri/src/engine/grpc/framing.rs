//! gRPC wire framing (distinct from HTTP/2 framing): every length-prefixed
//! gRPC message is 1 compression-flag byte + 4 big-endian length bytes +
//! payload. Re-homed from the 17c `#[cfg(test)]` spike into real module code
//! (#24) so the transport (#25) and drive loop (#28) can call `frame`/
//! `FrameUnframer` at runtime.
//!
//! Compression is unsupported for now (the flag byte is always 0). A future
//! task can extend `frame` to take a compress flag and add an inflate step in
//! `FrameUnframer` if compressed gRPC becomes a need; the framing layout
//! itself won't change.

/// Frames a single gRPC message payload: compression flag `0` (uncompressed)
/// + 4-byte big-endian length + payload.
#[allow(dead_code)] // callers land in #25 (transport) / #28 (drive loop)
pub(crate) fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(0u8);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Incremental unwrapper mirroring `sse::FrameParser`'s feed-as-it-comes
/// shape — real h2 DATA frames won't align to gRPC message boundaries, so the
/// transport feeds bytes as they arrive and this yields any fully-buffered
/// gRPC messages. Returns the decoded payloads (compression flag byte is
/// currently ignored — see module docs).
#[derive(Default)]
#[allow(dead_code)] // callers land in #25 (transport) / #28 (drive loop)
pub(crate) struct FrameUnframer {
    buf: Vec<u8>,
}

impl FrameUnframer {
    #[allow(dead_code)] // wired up by #25 (transport) / #28 (drive loop)
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 5 {
                break;
            }
            let len =
                u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
            if self.buf.len() < 5 + len {
                break;
            }
            let consumed: Vec<u8> = self.buf.drain(..5 + len).collect();
            out.push(consumed[5..].to_vec());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::grpc::testsupport::reflection_request_descriptor;
    use prost::Message as _;
    use prost_reflect::{DynamicMessage, Value};

    #[test]
    fn length_prefix_framing_matches_known_byte_layout() {
        let framed = frame(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(&framed[..5], &[0x00, 0x00, 0x00, 0x00, 0x03]);
        assert_eq!(&framed[5..], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn length_prefix_framing_round_trips_dynamic_message_bytes() {
        let desc = reflection_request_descriptor();
        let mut msg = DynamicMessage::new(desc);
        msg.set_field_by_name("host", Value::String("round-trip.example".to_string()));
        let payload = msg.encode_to_vec();

        let framed = frame(&payload);
        let mut unframer = FrameUnframer::default();
        let frames = unframer.feed(&framed);

        assert_eq!(frames, vec![payload]);
    }

    #[test]
    fn length_prefix_framing_handles_partial_frame_split_across_reads() {
        let framed = frame(&[1, 2, 3]);
        let (header, payload) = framed.split_at(5);

        let mut unframer = FrameUnframer::default();
        assert!(
            unframer.feed(header).is_empty(),
            "must not fire before the payload arrives"
        );
        let frames = unframer.feed(payload);
        assert_eq!(frames, vec![vec![1, 2, 3]]);
    }
}