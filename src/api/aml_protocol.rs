//! Pure AML identity contract. No storage, service, or native-engine dependency.
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Frozen namespace for AML v1 owned-workspace names. Changing it changes every
/// target workspace. This UUID is a public identifier, not an authorization secret.
pub const WORKSPACE_NAMESPACE: Uuid = Uuid::from_u128(0x9b6adf90_8252_4a40_83e0_64f14f6febed);
const WORKSPACE_ENCODING_V1: &[u8] = b"origence:owned-workspace:v1\0";

/// Derive from the authenticated tenant/root and the complete raw user ID.
/// IDs are neither truncated nor Unicode-normalized. Session, request, key, and
/// processing configuration deliberately do not participate in this mapping.
/// Authorization and checking the immutable owner relation remain service work.
pub fn workspace_id(tenant_id: Uuid, root_workspace_id: Uuid, user_id: &str) -> Uuid {
    Uuid::new_v5(
        &WORKSPACE_NAMESPACE,
        &workspace_name(tenant_id, root_workspace_id, user_id),
    )
}

fn workspace_name(tenant_id: Uuid, root_workspace_id: Uuid, user_id: &str) -> Vec<u8> {
    let tenant = tenant_id.to_string();
    let root = root_workspace_id.to_string();
    let mut name = WORKSPACE_ENCODING_V1.to_vec();
    for field in ["aml", tenant.as_str(), root.as_str(), user_id] {
        // UUID strings are canonical lowercase hyphenated UTF-8; the user field
        // is original UTF-8. All lengths count bytes, using unsigned u64 BE.
        name.extend_from_slice(&(field.len() as u64).to_be_bytes());
        name.extend_from_slice(field.as_bytes());
    }
    name
}

/// Bound the public command key without truncating the external batch ID. The
/// complete request ID is also retained in the generic batch request fingerprint.
pub fn add_command_key(request_id: &str) -> String {
    format!("aml:add:v1:{:x}", Sha256::digest(request_id.as_bytes()))
}
