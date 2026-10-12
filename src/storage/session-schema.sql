CREATE TABLE oc_session_turns (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    source_event_id BLOB NOT NULL, request_hash TEXT NOT NULL,
    created_by BLOB NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id),
    UNIQUE (tenant_id, workspace_id, source_event_id)
);
CREATE TABLE oc_session_turn_evidence (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    evidence_ref_id BLOB NOT NULL, ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence TEXT NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id, evidence_ref_id),
    UNIQUE (tenant_id, workspace_id, session_id, turn_id, ordinal)
);
CREATE TABLE oc_feedback (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    principal_id BLOB NOT NULL, feedback_id BLOB NOT NULL,
    session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    request_hash TEXT NOT NULL, feedback TEXT NOT NULL, created_at INTEGER NOT NULL,
    feedback_sequence INTEGER NOT NULL CHECK (feedback_sequence > 0),
    PRIMARY KEY (tenant_id, workspace_id, principal_id, feedback_id),
    UNIQUE (tenant_id, workspace_id, feedback_sequence)
);
CREATE INDEX oc_feedback_turn ON oc_feedback(tenant_id, workspace_id, session_id, turn_id);
