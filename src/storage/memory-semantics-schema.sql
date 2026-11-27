CREATE TABLE oc_memory_properties (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, predicate TEXT NOT NULL,
    definition TEXT NOT NULL, created_by BLOB NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, predicate),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);

CREATE TABLE oc_long_term_memories (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    identity_key TEXT NOT NULL, identity_json TEXT NOT NULL,
    current_version INTEGER NOT NULL DEFAULT 0 CHECK (current_version >= 0),
    PRIMARY KEY (tenant_id, workspace_id, id),
    UNIQUE (tenant_id, workspace_id, identity_key),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);

CREATE TABLE oc_long_term_memory_versions (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, memory_id BLOB NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, memory_id, version),
    FOREIGN KEY (tenant_id, workspace_id, memory_id)
        REFERENCES oc_long_term_memories(tenant_id, workspace_id, id)
);

-- The quote and its source never change. The half-open version interval records
-- when that evidence supported a derived memory, including source withdrawal.
CREATE TABLE oc_long_term_memory_facts (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, source_event_id BLOB NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0), memory_id BLOB NOT NULL,
    statement TEXT NOT NULL, effective_at_ms INTEGER,
    introduced_version INTEGER NOT NULL, withdrawn_version INTEGER,
    CHECK (withdrawn_version IS NULL OR withdrawn_version > introduced_version),
    PRIMARY KEY (tenant_id, workspace_id, source_event_id, ordinal),
    FOREIGN KEY (tenant_id, workspace_id, source_event_id)
        REFERENCES oc_events(tenant_id, workspace_id, id),
    FOREIGN KEY (tenant_id, workspace_id, memory_id, introduced_version)
        REFERENCES oc_long_term_memory_versions(tenant_id, workspace_id, memory_id, version),
    FOREIGN KEY (tenant_id, workspace_id, memory_id, withdrawn_version)
        REFERENCES oc_long_term_memory_versions(tenant_id, workspace_id, memory_id, version)
);

CREATE INDEX oc_long_term_memory_fact_history ON oc_long_term_memory_facts(
    tenant_id, workspace_id, memory_id, introduced_version, source_event_id, ordinal
);
CREATE INDEX oc_long_term_memory_fact_live ON oc_long_term_memory_facts(
    tenant_id, workspace_id, memory_id, introduced_version, source_event_id, ordinal
) WHERE withdrawn_version IS NULL;
