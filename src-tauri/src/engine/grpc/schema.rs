//! Runtime `.proto` upload + compile via `protox`, building a
//! `prost_reflect::DescriptorPool`. Enforces local-only import resolution (no
//! network fetch — same posture as the OpenAPI no-external-`$ref` rule). Lands
//! as task #27. Stub for now; filled in by the #27 task.