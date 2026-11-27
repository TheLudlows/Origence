//! HTTP protocol adapter for the local host.
mod deployment;
mod foundation;
mod global_knowledge;
mod http;
mod mcp_http;
pub use http::{router, router_with_mcp};
pub use mcp_http::McpHttpConfig;
