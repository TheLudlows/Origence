CREATE TABLE oc_aml_adds (
    tenant_id BLOB NOT NULL,
    workspace_id BLOB NOT NULL,
    request_id TEXT NOT NULL COLLATE BINARY,
    session_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    embedding_profile TEXT NOT NULL,
    asset_id BLOB NOT NULL,
    source_event_id BLOB NOT NULL,
    job_id BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, request_id),
    UNIQUE (tenant_id, workspace_id, job_id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_aml_users(tenant_id, workspace_id),
    FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, job_id) REFERENCES oc_jobs(tenant_id, workspace_id, id)
);
