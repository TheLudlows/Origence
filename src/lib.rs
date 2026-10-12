pub mod aml;
#[cfg(feature = "local-storage")]
pub mod api;
pub mod capture;
pub mod error;
pub mod evidence;
#[cfg(feature = "local-storage")]
mod context_retrieval;
#[cfg(feature = "local-storage")]
mod context;
pub mod graph;
#[cfg(feature = "local-storage")]
mod improvements;
#[cfg(any(feature = "local-storage", test))]
mod jobs;
pub mod learning;
#[cfg(feature = "local-storage")]
pub mod mcp;
pub mod memory_identity;
pub mod memory_semantics;
pub mod models;
pub mod parsing;
#[cfg(feature = "local-storage")]
pub mod retrieval;
pub mod semantic_capture;
#[cfg(feature = "local-storage")]
mod semantics;
#[cfg(feature = "local-storage")]
pub mod service;
pub mod session;
#[cfg(feature = "local-storage")]
mod sessions;
pub mod storage;
pub mod types;
#[cfg(feature = "local-storage")]
pub mod worker;

#[cfg(feature = "local-storage")]
pub mod client;
#[cfg(feature = "local-storage")]
pub mod host;
