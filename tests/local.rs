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
mod local_blob;
mod local_ledger;
mod message_batches;
mod owned_workspace_service;
mod owned_workspaces;
mod pdf_parsing;
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
mod workspace_schema_rejection;
