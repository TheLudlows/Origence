CREATE TABLE oc_memory_identities (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL,
    identity_key TEXT NOT NULL, asset_id BLOB NOT NULL,
    identity_json TEXT NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, identity_key),
    UNIQUE (tenant_id, workspace_id, asset_id),
    FOREIGN KEY (tenant_id, workspace_id, asset_id)
        REFERENCES oc_assets(tenant_id, workspace_id, id)
);
