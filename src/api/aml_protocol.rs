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

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT: &str = "00112233-4455-6677-8899-aabbccddeeff";
    const ROOT: &str = "10213243-5465-7687-98a9-bacbdcedfe0f";

    #[test]
    fn published_cross_language_vectors_match_uuid_library_and_encoding() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../docs/contracts/aml-workspace-v1-vectors.json"
        ))
        .unwrap();
        assert_eq!(contract["namespace"], WORKSPACE_NAMESPACE.to_string());
        let vectors = contract["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 8);
        for vector in vectors {
            let tenant = Uuid::parse_str(vector["tenant"].as_str().unwrap()).unwrap();
            let root = Uuid::parse_str(vector["root"].as_str().unwrap()).unwrap();
            let user = vector["user"].as_str().unwrap();
            let name = workspace_name(tenant, root, user);
            let hex: String = name.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(hex, vector["input_hex"].as_str().unwrap());
            assert_eq!(
                workspace_id(tenant, root, user).to_string(),
                vector["workspace_id"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn frozen_uuid_v5_vectors_preserve_all_identity_bytes() {
        let tenant = Uuid::parse_str(TENANT).unwrap();
        let root = Uuid::parse_str(ROOT).unwrap();
        for (user, expected) in [
            ("user", "a4eb49d8-1549-5ea2-a64e-76b0eb417e0d"),
            ("User", "bf1c7d75-7467-5a18-95a8-41f837d2dc89"),
            ("用户🙂", "39baa865-6859-51d7-8d62-6c86953e2041"),
            ("a:b|c\n", "fc3cfbe0-63a1-5d2c-9f9b-9f4edfdae3d7"),
            ("é", "7a18994a-7e45-5a99-8ed4-f4b9ffab262f"),
            ("e\u{301}", "e075c3c2-a007-5036-8f0b-8ea6e15b4588"),
        ] {
            let id = workspace_id(tenant, root, user);
            assert_eq!(id.to_string(), expected);
            assert_eq!(id.get_version_num(), 5);
            assert_eq!(workspace_id(tenant, root, user), id);
        }
        assert_eq!(
            workspace_id(
                Uuid::parse_str("00112233-4455-6677-8899-aabbccddee00").unwrap(),
                root,
                "user",
            )
            .to_string(),
            "d79e684c-c9fd-5fad-a769-383cc2445aad"
        );
        assert_eq!(
            workspace_id(
                tenant,
                Uuid::parse_str("10213243-5465-7687-98a9-bacbdcedfe00").unwrap(),
                "user",
            )
            .to_string(),
            "6f8b5cf5-2309-5372-b094-6437a848dd1f"
        );
    }

    #[test]
    fn frozen_name_encoding_is_byte_length_prefixed_and_versioned() {
        let name = workspace_name(
            Uuid::parse_str(TENANT).unwrap(),
            Uuid::parse_str(ROOT).unwrap(),
            "用户🙂",
        );
        let actual: String = name.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            actual,
            concat!(
                "6f726967656e63653a6f776e65642d776f726b73706163653a763100",
                "0000000000000003616d6c",
                "000000000000002430303131323233332d343435352d363637372d383839392d616162626363646465656666",
                "000000000000002431303231333234332d353436352d373638372d393861392d626163626463656466653066",
                "000000000000000ae794a8e688b7f09f9982"
            )
        );
        let full_user = "长".repeat(1365);
        assert_ne!(
            workspace_id(Uuid::nil(), Uuid::nil(), &full_user),
            workspace_id(Uuid::nil(), Uuid::nil(), &(full_user + "a"))
        );
    }

    #[test]
    fn frozen_request_keys_hash_complete_raw_utf8_with_adapter_prefix() {
        for (request, expected) in [
            (
                "request".to_owned(),
                "aml:add:v1:1f58b9145b24d108d7ac38887338b3ea3229833b9c1e418250343f907bfd1047",
            ),
            (
                "请求🙂".to_owned(),
                "aml:add:v1:afd524f8ca555d3c381928042f731a81d2f8afe364c3f64e848e3a3fce849b21",
            ),
            (
                "a".repeat(4096),
                "aml:add:v1:c93eee2d0db02f10acc7460d9576e122dcf8cd53c4bf8dfcae1b3e74ebcfff5a",
            ),
        ] {
            let key = add_command_key(&request);
            assert_eq!(key, expected);
            assert!(key.is_ascii());
            assert!(key.len() <= 200);
        }
        assert_ne!(add_command_key("é"), add_command_key("e\u{301}"));
    }
}
