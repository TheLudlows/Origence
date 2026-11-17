CREATE TABLE oc_skill_candidates (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    owner_key_id BLOB NOT NULL, current_revision INTEGER NOT NULL CHECK(current_revision > 0),
    PRIMARY KEY(tenant_id,workspace_id,id)
);
CREATE TABLE oc_skill_revisions (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, candidate_id BLOB NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0), content TEXT NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,workspace_id,candidate_id,revision)
);
CREATE TABLE oc_skill_reviews (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, id BLOB NOT NULL,
    candidate_id BLOB NOT NULL, revision INTEGER NOT NULL,
    reviewer_id BLOB NOT NULL, packet TEXT NOT NULL, digest TEXT NOT NULL,
    decision TEXT NOT NULL DEFAULT 'pending' CHECK(decision IN ('pending','approved','rejected')),
    decided_at INTEGER, created_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,workspace_id,id)
);
CREATE TABLE oc_skill_packages (
    tenant_id BLOB NOT NULL, workspace_id BLOB NOT NULL, skill_id BLOB NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0), review_id BLOB NOT NULL,
    owner_key_id BLOB NOT NULL, sources TEXT NOT NULL, package TEXT NOT NULL,
    withdrawn INTEGER NOT NULL DEFAULT 0 CHECK(withdrawn IN (0,1)), created_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,workspace_id,skill_id,version),
    UNIQUE(tenant_id,workspace_id,review_id)
);
