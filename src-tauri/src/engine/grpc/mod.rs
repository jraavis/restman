//! gRPC dynamic client (Phase 6 task 4, sub-phase 17d). Sub-modules:
//! - `framing`: gRPC length-prefix (de)framing — re-homed from the 17c
//!   throwaway spike into real module code (landed with #24).
//! - `transport` (#25), `reflection` (#26), `schema` (#27): land in their own
//!   files as the corresponding sub-tasks complete.
//!
//! This module replaces the 17c spike (`engine/grpc.rs`). The protox/
//! prost-reflect compilation tests below stay here under `#[cfg(test)]`; the
//! framing helpers and their tests moved to `framing.rs`.

pub(crate) mod framing;
pub(crate) mod reflection;
pub(crate) mod schema;
pub(crate) mod transport;

#[cfg(test)]
mod testsupport;

#[cfg(test)]
mod tests {
    use super::testsupport::{compile_reflection_proto, reflection_request_descriptor, FIXTURES_ROOT};
    use prost::Message as _;
    use prost_reflect::{DynamicMessage, Value};

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
        // Negative case proving import resolution fails closed on a local
        // lookup miss rather than (say) trying the network; see the
        // `cargo tree` check in PLAN.md #17c.
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
}