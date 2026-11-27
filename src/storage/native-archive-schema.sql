CREATE TABLE oc_native_archive_sessions (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, session_id BLOB NOT NULL,
    last_sequence INTEGER NOT NULL CHECK (last_sequence > 0),
    cleanup_after INTEGER,
    PRIMARY KEY (tenant_id, workspace_id, session_id)
);
CREATE INDEX oc_native_archive_sessions_cleanup ON oc_native_archive_sessions(cleanup_after);
CREATE TABLE oc_native_archive_proofs (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    evidence_reported INTEGER NOT NULL CHECK (evidence_reported IN (0,1)),
    body_bytes INTEGER NOT NULL CHECK (body_bytes > 0),
    PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id),
    FOREIGN KEY (tenant_id, workspace_id, session_id, turn_id)
        REFERENCES oc_session_turns(tenant_id,workspace_id,session_id,turn_id)
);
CREATE TABLE oc_native_archive_withdrawals (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    cleanup_after INTEGER,
    PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id)
);
CREATE INDEX oc_native_archive_withdrawals_cleanup ON oc_native_archive_withdrawals(cleanup_after);
