//! gRPC feasibility spike (Phase 6 task 4, sub-phase 17c). Throwaway spike,
//! not shipped code: proves `protox` + `prost-reflect::DynamicMessage` + a
//! hand-rolled gRPC length-prefix framer compose correctly, against a
//! checked-in `FileDescriptorSet` fixture, before 17d builds a dynamic gRPC
//! client UI on top of this stack.
//!
//! Deliberately offline only — no h2/TLS socket, no live server. That half
//! (the actual gRPC-over-HTTP/2 transport, including trailers) is unproven by
//! this spike and is 17d's problem in full. Nothing here is `pub`: this
//! module exists solely for the tests below; 17d re-homes any reusable
//! pieces into real module code (and promotes `protox`/`prost-reflect`/
//! `prost` to `[dependencies]`) when it needs them at runtime.

#[cfg(test)]
mod tests {
    use prost::Message as _;
    use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};

    const FIXTURES_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/grpc");

    fn compile_reflection_proto() -> prost_types::FileDescriptorSet {
        protox::compile(["reflection.proto"], [FIXTURES_ROOT])
            .expect("reflection.proto should compile (it has no imports)")
    }

    fn reflection_request_descriptor() -> MessageDescriptor {
        let fds = compile_reflection_proto();
        let pool = DescriptorPool::from_file_descriptor_set(fds)
            .expect("compiled descriptor set should be valid");
        pool.get_message_by_name("grpc.reflection.v1.ServerReflectionRequest")
            .expect("ServerReflectionRequest should be present in reflection.proto")
    }

    #[test]
    fn protox_compiles_self_contained_reflection_proto() {
        let fds = compile_reflection_proto();
        assert!(fds
            .file
            .iter()
            .any(|f| f.name.as_deref() == Some("reflection.proto")));
    }

    #[test]
    fn protox_resolves_relative_import_within_include_dir() {
        // importer/main.proto imports "common/shared.proto" — both fixtures
        // sit under one include root, so this must compile clean.
        let fds = protox::compile(["importer/main.proto"], [FIXTURES_ROOT])
            .expect("import should resolve when common/ is under the same include root");
        let names: Vec<&str> = fds.file.iter().filter_map(|f| f.name.as_deref()).collect();
        assert!(names.contains(&"importer/main.proto"));
        assert!(names.contains(&"common/shared.proto"));
    }

    #[test]
    fn protox_errors_on_missing_import_rather_than_hanging() {
        // Same importer/main.proto, but the include root is narrowed to just
        // the importer/ directory — "common/shared.proto" can't be found.
        // This is the negative case proving import resolution fails closed
        // on a local lookup miss rather than (say) trying the network; see
        // the `cargo tree` check in PLAN.md #17c for the direct evidence that
        // no network fetch is even reachable from this dependency tree.
        let narrow_root = format!("{FIXTURES_ROOT}/importer");
        let result = protox::compile(["main.proto"], [narrow_root]);
        assert!(
            result.is_err(),
            "expected a clean Err when common/shared.proto isn't reachable, got Ok"
        );
    }

    #[test]
    fn descriptor_pool_builds_from_compiled_file_descriptor_set() {
        let desc = reflection_request_descriptor();
        assert_eq!(
            desc.full_name(),
            "grpc.reflection.v1.ServerReflectionRequest"
        );
    }

    #[test]
    fn dynamic_message_round_trips_through_encode_decode() {
        let desc = reflection_request_descriptor();

        let mut msg = DynamicMessage::new(desc.clone());
        msg.set_field_by_name("host", Value::String("example.com".to_string()));
        msg.set_field_by_name("list_services", Value::String("*".to_string()));

        let bytes = msg.encode_to_vec();
        let decoded = DynamicMessage::decode(desc, bytes.as_slice())
            .expect("encoded DynamicMessage bytes should decode cleanly");

        assert_eq!(
            decoded.get_field_by_name("host").unwrap().as_str(),
            Some("example.com")
        );
        assert_eq!(
            decoded.get_field_by_name("list_services").unwrap().as_str(),
            Some("*")
        );
    }

    /// gRPC wire framing (distinct from HTTP/2 framing): 1-byte compression
    /// flag + 4-byte big-endian length + payload.
    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + payload.len());
        out.push(0u8);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Incremental unwrapper, mirroring `sse::FrameParser`'s feed-as-it-comes
    /// shape — real h2 DATA frames won't align to gRPC message boundaries.
    #[derive(Default)]
    struct FrameUnframer {
        buf: Vec<u8>,
    }

    impl FrameUnframer {
        fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
            self.buf.extend_from_slice(bytes);
            let mut out = Vec::new();
            loop {
                if self.buf.len() < 5 {
                    break;
                }
                let len =
                    u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]])
                        as usize;
                if self.buf.len() < 5 + len {
                    break;
                }
                let consumed: Vec<u8> = self.buf.drain(..5 + len).collect();
                out.push(consumed[5..].to_vec());
            }
            out
        }
    }

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
