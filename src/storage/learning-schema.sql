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
