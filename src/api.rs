//! HTTP protocol adapter for the local host.
mod foundation;
mod http;
mod mcp_http;
pub use http::{router, router_with_mcp};
pub use mcp_http::McpHttpConfig;
