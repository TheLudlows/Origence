CREATE TABLE oc_runs (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, run_id BLOB NOT NULL,
    session_id BLOB NOT NULL, owner_key_id BLOB NOT NULL,
    input TEXT NOT NULL, input_hash TEXT NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,workspace_id,run_id)
);
CREATE TABLE oc_run_events (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, run_id BLOB NOT NULL,
    event_id BLOB NOT NULL, sequence INTEGER NOT NULL CHECK(sequence>0),
    reporter_principal_id BLOB NOT NULL, payload TEXT NOT NULL,
    payload_hash TEXT NOT NULL, references_json TEXT NOT NULL, recorded_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,workspace_id,run_id,event_id),
    UNIQUE(tenant_id,workspace_id,run_id,sequence)
);
