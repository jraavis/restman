//! Network engine. HTTP client (reqwest) lives here; SSE landed in Phase 6
//! task #17a, WebSocket in #17b. gRPC lands in later #17 sub-phases.

pub mod http;
mod grpc;
pub mod sse;
pub mod ws;
