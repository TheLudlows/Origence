#[cfg(feature = "host")]
mod catalog;
#[cfg(feature = "host")]
mod client_hooks;
#[cfg(feature = "host")]
mod content_storage;
#[cfg(feature = "host")]
mod support;
// One integration executable avoids repeatedly linking the native engines.
mod evidence_bundle;
#[cfg(feature = "host")]
mod foundation_http;
#[cfg(feature = "host")]
mod global_knowledge;
mod identity_check_cli;
mod identity_vector_regression;
mod initialization;
mod learning_loop;
mod local_app;
mod local_ledger;
mod message_batches;
mod production_storage;
mod semantic_capture;
mod semantic_catalog;
mod semantic_memory;
mod session_feedback;
mod sqlite_domain;
mod sqlite_evidence;
mod sqlite_identity;
mod sqlite_identity_check;
mod sqlite_ledger;
mod sqlite_lock;
mod sqlite_owner;
mod sqlite_store;
mod sqlite_visibility;
mod storage_contract;
mod storage_recovery;
#[cfg(feature = "host")]
mod workspace_access;
mod workspace_commands;
mod workspace_isolation;
mod workspace_schema_rejection;

#[cfg(feature = "host")]
mod pdf_fixture;
#[cfg(feature = "host")]
mod retrieval_diagnostics;

#[cfg(feature = "host")]
mod mcp_http;
#[cfg(feature = "host")]
mod mcp_runs;

#[cfg(feature = "host")]
mod run_pdf_source;

#[cfg(feature = "host")]
mod turn_sync;
