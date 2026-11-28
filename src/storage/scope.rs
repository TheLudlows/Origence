//! Tenant/workspace scope and authorization types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The `tenant_id` + `workspace_id` pair that scopes every stored object.
///
/// Every business operation carries this explicitly; it can never be omitted
/// or defaulted (A2.4). Object IDs, unique constraints, owners, caches and
/// blob keys are all prefixed with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
}

/// A principal that has authenticated against a [`Scope`].
///
/// Produced by token authentication or an authorized workspace lookup.
/// Transactions recheck the live key and any persisted ownership relationship;
/// caller-supplied scope/role values do not grant access (A2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedScope {
    pub scope: Scope,
    pub principal_id: Uuid,
    pub role: String,
}

/// A workspace explicitly available to an authenticated credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAccess {
    pub workspace_id: Uuid,
    pub name: String,
    pub role: String,
    pub is_default: bool,
}

/// A permission a domain operation can require (A2.2 `check_current_permission`).
///
/// Workspace keys and the deployment administrator can manage workspace content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    Read,
    Write,
    Delete,
    InspectAgents,
}

impl Permission {
    /// The permission name used by the role matrix and audit records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::InspectAgents => "inspect_agents",
        }
    }

    /// Whether the verified identity permits this operation.
    pub fn granted_by(self, role: &str) -> bool {
        matches!(role, "workspace" | "deployment_admin")
    }
}

/// A specific version of a source (event/asset) that graph and vector records
/// originate from. Graph/vector writes must carry it so visibility can be
/// re-verified against the relational ledger (A2.2, A2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceVersion {
    pub source_id: Uuid,
    pub version: i32,
}
