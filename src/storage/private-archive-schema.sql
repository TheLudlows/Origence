CREATE TABLE oc_private_archives (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    logical_key_id BLOB NOT NULL, session_id BLOB NOT NULL, turn_id BLOB NOT NULL,
    source_event_id BLOB NOT NULL, group_id TEXT NOT NULL,
    body_hash TEXT NOT NULL, body_bytes INTEGER NOT NULL, operation_epoch INTEGER NOT NULL,
    evidence_reported INTEGER NOT NULL CHECK (evidence_reported IN (0,1)),
    turn_sequence INTEGER NOT NULL CHECK (turn_sequence > 0),
    state TEXT NOT NULL CHECK (state IN ('pending','archived','withdrawn')),
    confirmed_seq INTEGER, cleanup_done INTEGER NOT NULL DEFAULT 0,
    cleanup_after INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, workspace_id, session_id, turn_id),
    UNIQUE (tenant_id, workspace_id, source_event_id),
    UNIQUE (tenant_id, workspace_id, session_id, turn_sequence),
    UNIQUE (tenant_id, workspace_id, logical_key_id, group_id)
);
CREATE INDEX oc_private_archives_cleanup ON oc_private_archives(state, cleanup_done, cleanup_after);
CREATE TABLE oc_withdrawn_private_sessions (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, session_id BLOB NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, session_id)
);
CREATE TABLE oc_private_archive_migration (
    id INTEGER PRIMARY KEY CHECK (id=1),
    phase TEXT NOT NULL CHECK (phase IN ('copying','purging','complete'))
);
CREATE TABLE oc_archive_legacy_run_prefixes (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, run_id BLOB NOT NULL,
    through_sequence INTEGER NOT NULL, digest TEXT NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, run_id, through_sequence)
);
