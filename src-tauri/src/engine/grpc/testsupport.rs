//! Test-only helpers shared across the gRPC sub-modules' `#[cfg(test)]`
//! blocks (compile reflection.proto, build a `MessageDescriptor`). Kept in a
//! cfg-gated file so a normal `cargo build` compiles it to nothing — true to
//! the "throwaway spike" posture of 17c that 17d inherits for test fixtures.

use prost_reflect::{DescriptorPool, MessageDescriptor};

pub(crate) const FIXTURES_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/grpc");

pub(crate) fn compile_reflection_proto() -> prost_types::FileDescriptorSet {
    protox::compile(["reflection.proto"], [FIXTURES_ROOT])
        .expect("reflection.proto should compile (it has no imports)")
}

pub(crate) fn reflection_request_descriptor() -> MessageDescriptor {
    let fds = compile_reflection_proto();
    let pool = DescriptorPool::from_file_descriptor_set(fds)
        .expect("compiled descriptor set should be valid");
    pool.get_message_by_name("grpc.reflection.v1.ServerReflectionRequest")
        .expect("ServerReflectionRequest should be present in reflection.proto")
}