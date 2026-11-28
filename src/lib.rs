#[cfg(feature = "host")]
mod agent_presence;
#[cfg(feature = "host")]
pub mod api;
#[cfg(feature = "host")]
pub mod archive_maintenance;
pub mod capture;
#[cfg(feature = "host")]
pub mod consolidation;
#[cfg(feature = "host")]
mod context;
#[cfg(feature = "host")]
mod context_retrieval;
pub mod error;
pub mod evidence;
pub mod graph;
#[cfg(feature = "host")]
mod improvements;
#[cfg(any(feature = "host", test))]
mod jobs;
pub mod learning;
#[cfg(feature = "host")]
pub mod long_term_memory;
#[cfg(feature = "host")]
mod mcp;
pub mod memory_identity;
pub mod memory_semantics;
pub mod memory_timeline;
pub mod messages;
pub mod models;
pub mod parsing;
#[cfg(feature = "host")]
pub mod retrieval;
pub mod retrieval_diagnostics;
pub mod run;
#[cfg(feature = "host")]
pub mod run_pdf;
pub mod run_verification;
#[cfg(feature = "host")]
mod runs;
pub mod semantic_capture;
#[cfg(feature = "host")]
mod semantics;
#[cfg(feature = "host")]
pub mod service;
pub mod session;
#[cfg(feature = "host")]
mod sessions;
pub mod skill;
pub mod skill_candidate;
#[cfg(feature = "host")]
pub mod skill_workflow;
#[cfg(feature = "host")]
mod skills;
pub mod storage;
pub mod types;
#[cfg(feature = "host")]
pub mod worker;

#[cfg(feature = "host")]
pub mod client;
#[cfg(feature = "host")]
pub mod client_hook;
#[cfg(feature = "host")]
pub mod client_skill;
#[cfg(feature = "host")]
pub mod host;
