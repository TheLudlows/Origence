CREATE TABLE oc_memory_properties (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, predicate TEXT NOT NULL,
    definition TEXT NOT NULL, created_by BLOB NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, predicate),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);
