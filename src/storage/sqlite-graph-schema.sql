-- Graph tables backing SqliteGraphStore (M4). DDL, keys, cascades and indexes
-- are validated by Lifecycle::check, never repaired at runtime (A2.3).
CREATE TABLE IF NOT EXISTS oc_graph_entities (
    uid TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    description TEXT NOT NULL,
    source_id TEXT NOT NULL,
    version INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS oc_graph_entities_scope
    ON oc_graph_entities(tenant_id, workspace_id);
CREATE INDEX IF NOT EXISTS oc_graph_entities_source
    ON oc_graph_entities(tenant_id, workspace_id, source_id, version);

CREATE TABLE IF NOT EXISTS oc_graph_relations (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    head_uid TEXT NOT NULL,
    tail_uid TEXT NOT NULL,
    predicate TEXT NOT NULL,
    source_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (head_uid, tail_uid, id),
    FOREIGN KEY (head_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE,
    FOREIGN KEY (tail_uid) REFERENCES oc_graph_entities(uid) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS oc_graph_relations_scope
    ON oc_graph_relations(tenant_id, workspace_id);
CREATE INDEX IF NOT EXISTS oc_graph_relations_head
    ON oc_graph_relations(head_uid);
CREATE INDEX IF NOT EXISTS oc_graph_relations_source
    ON oc_graph_relations(tenant_id, workspace_id, source_id, version);
CREATE INDEX IF NOT EXISTS oc_graph_relations_by_id
    ON oc_graph_relations(tenant_id, workspace_id, id);
CREATE INDEX IF NOT EXISTS oc_graph_relations_tail
    ON oc_graph_relations(tail_uid);
