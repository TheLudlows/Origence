CREATE TABLE oc_aml_namespaces (
    tenant_id BLOB NOT NULL,
    workspace_id BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);

CREATE TABLE oc_aml_users (
    tenant_id BLOB NOT NULL,
    namespace_workspace_id BLOB NOT NULL,
    user_id TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(user_id AS BLOB)) BETWEEN 1 AND 4096),
    workspace_id BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, namespace_workspace_id, user_id),
    UNIQUE (tenant_id, workspace_id),
    CHECK (namespace_workspace_id != workspace_id),
    FOREIGN KEY (tenant_id, namespace_workspace_id) REFERENCES oc_aml_namespaces(tenant_id, workspace_id),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES oc_workspaces(tenant_id, id)
);
