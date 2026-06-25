//! Network engine. HTTP client (reqwest) lives here; SSE landed in Phase 6
//! task #17a. WebSocket/gRPC land in later #17 sub-phases.

pub mod http;
pub mod sse;
