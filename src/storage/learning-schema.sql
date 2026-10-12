CREATE TABLE oc_improve_cursors (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, session_id BLOB NOT NULL,
    feedback_sequence INTEGER NOT NULL DEFAULT 0 CHECK (feedback_sequence >= 0),
    PRIMARY KEY (tenant_id, workspace_id, session_id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);
CREATE TABLE oc_improve_runs (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, improve_id BLOB NOT NULL,
    session_id BLOB NOT NULL, job_id BLOB NOT NULL,
    request_hash TEXT NOT NULL, request TEXT NOT NULL, snapshot TEXT NOT NULL,
    lower_sequence INTEGER NOT NULL CHECK (lower_sequence >= 0),
    target_sequence INTEGER NOT NULL CHECK (target_sequence >= lower_sequence),
    proposal_plan TEXT, verification_plan TEXT,
    status TEXT NOT NULL DEFAULT 'accepted' CHECK (status IN ('accepted','publishing','completed','cancelled')),
    created_by BLOB NOT NULL, created_at INTEGER NOT NULL, completed_at INTEGER,
    PRIMARY KEY (tenant_id, workspace_id, improve_id),
    UNIQUE (tenant_id, workspace_id, job_id),
    FOREIGN KEY (tenant_id, workspace_id, session_id) REFERENCES oc_improve_cursors(tenant_id, workspace_id, session_id),
    FOREIGN KEY (tenant_id, workspace_id, job_id) REFERENCES oc_jobs(tenant_id, workspace_id, id) DEFERRABLE INITIALLY DEFERRED
);
CREATE UNIQUE INDEX oc_improve_unfinished ON oc_improve_runs(tenant_id, workspace_id, session_id) WHERE status IN ('accepted','publishing');
CREATE TABLE oc_learnings (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    improve_id BLOB NOT NULL, session_id BLOB NOT NULL, candidate_index INTEGER NOT NULL CHECK (candidate_index >= 0 AND candidate_index < 8),
    asset_id BLOB NOT NULL, source_event_id BLOB NOT NULL, ingest_job_id BLOB NOT NULL,
    candidate TEXT NOT NULL, fingerprint TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'accepted' CHECK (status IN ('accepted','published')),
    created_by BLOB NOT NULL, created_at INTEGER NOT NULL, published_at INTEGER,
    PRIMARY KEY (tenant_id, workspace_id, id),
    UNIQUE (tenant_id, workspace_id, improve_id, candidate_index),
    UNIQUE (tenant_id, workspace_id, asset_id),
    UNIQUE (tenant_id, workspace_id, source_event_id),
    UNIQUE (tenant_id, workspace_id, ingest_job_id),
    FOREIGN KEY (tenant_id, workspace_id, improve_id) REFERENCES oc_improve_runs(tenant_id, workspace_id, improve_id),
    FOREIGN KEY (tenant_id, workspace_id, asset_id) REFERENCES oc_assets(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, source_event_id) REFERENCES oc_events(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, ingest_job_id) REFERENCES oc_jobs(tenant_id, workspace_id, id)
);
CREATE TABLE oc_learning_support (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, learning_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    support_ordinal INTEGER NOT NULL CHECK (support_ordinal >= 0 AND support_ordinal < 32),
    principal_id BLOB NOT NULL, feedback_id BLOB NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, learning_id, support_ordinal),
    FOREIGN KEY (tenant_id, workspace_id, learning_id) REFERENCES oc_learnings(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, session_id, turn_id) REFERENCES oc_session_turns(tenant_id, workspace_id, session_id, turn_id),
    FOREIGN KEY (tenant_id, workspace_id, principal_id, feedback_id) REFERENCES oc_feedback(tenant_id, workspace_id, principal_id, feedback_id)
);
CREATE INDEX oc_learning_turn_support ON oc_learning_support(tenant_id, workspace_id, session_id, turn_id);
CREATE INDEX oc_learning_fingerprint ON oc_learnings(tenant_id, workspace_id, fingerprint);
