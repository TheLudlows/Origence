#[cfg(feature = "local-storage")]
mod catalog;
#[cfg(feature = "local-storage")]
mod client_hooks;
#[cfg(feature = "local-storage")]
mod content_storage;
#[cfg(feature = "local-storage")]
mod support;
// One integration executable avoids repeatedly linking the native engines.
mod evidence_bundle;
#[cfg(feature = "local-storage")]
mod foundation_http;
mod identity_check_cli;
mod identity_vector_regression;
mod initialization;
mod learning_loop;
mod local_app;
mod local_ledger;
mod message_batches;
mod owned_workspace_service;
mod owned_workspaces;
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
#[cfg(feature = "local-storage")]
mod workspace_access;
mod workspace_schema_rejection;

#[cfg(feature = "local-storage")]
mod pdf_fixture;
#[cfg(feature = "local-storage")]
mod retrieval_diagnostics;

#[cfg(feature = "local-storage")]
mod mcp_http;
#[cfg(feature = "local-storage")]
mod mcp_runs;

#[cfg(feature = "local-storage")]
mod run_pdf_source;
