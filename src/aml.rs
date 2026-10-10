//! Textual AML protocol and message-local provenance.
#[cfg(feature = "local-storage")]
mod service;

use crate::{
    error::{AppError, Result},
    parsing,
    types::Chunk,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub const MAX_MESSAGES: usize = 256;
pub const MAX_ID_BYTES: usize = 4096;
pub const ADD_WAIT: std::time::Duration = std::time::Duration::from_secs(25 * 60);
pub const MESSAGE_PARSER: &str = "aml-message-ranges-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AddInput {
    pub request_id: String,
    pub user_id: String,
    pub session_id: String,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddResponse {
    pub success: bool,
    pub request_id: String,
    pub user_id: String,
    pub session_id: String,
}

/// Durable receipt for library hosts; never an HTTP Add success response.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AddRecord {
    pub request_id: String,
    pub session_id: String,
    pub request_hash: String,
    pub embedding_profile: String,
    pub asset_id: Uuid,
    pub source_event_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    pub query: String,
    pub user_id: String,
    pub top_k: usize,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.contains('\0') {
        return Err(AppError::Invalid(
            "AML IDs must contain 1..4096 UTF-8 bytes without NUL".into(),
        ));
    }
    Ok(())
}
impl AddInput {
    pub fn validate(&self) -> Result<()> {
        for id in [&self.request_id, &self.user_id, &self.session_id] {
            validate_id(id)?;
        }
        if self.messages.is_empty() || self.messages.len() > MAX_MESSAGES {
            return Err(AppError::Invalid("AML Add requires 1..256 messages".into()));
        }
        let mut total = 0usize;
        for message in &self.messages {
            parsing::validate_text(&message.content)?;
            total = total.saturating_add(message.content.len());
            if total > parsing::MAX_TEXT {
                return Err(AppError::Invalid("AML message text exceeds 1 MB".into()));
            }
        }
        Ok(())
    }
}
impl SearchInput {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.user_id)?;
        if self.query.trim().is_empty()
            || self.query.len() > 4000
            || self.query.contains('\0')
            || !(1..=100).contains(&self.top_k)
        {
            return Err(AppError::Invalid("AML query or top_k invalid".into()));
        }
        Ok(())
    }
}

pub fn parse_add(bytes: &[u8]) -> Result<AddInput> {
    if bytes.len() > parsing::MAX_FILE {
        return Err(AppError::Invalid("AML request too large".into()));
    }
    let input: AddInput = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Invalid("invalid AML Add request".into()))?;
    input.validate()?;
    Ok(input)
}
pub fn parse_search(bytes: &[u8]) -> Result<SearchInput> {
    if bytes.len() > parsing::MAX_FILE {
        return Err(AppError::Invalid("AML request too large".into()));
    }
    let input: SearchInput = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Invalid("invalid AML Search request".into()))?;
    input.validate()?;
    Ok(input)
}

/// Off-executor parser used by the serialized worker. Bytes address decoded
/// message content at source_path, never the serialized JSON or joined asset.
pub fn chunks(source: &str) -> Result<Vec<Chunk>> {
    let input = parse_add(source.as_bytes())?;
    let count = input.messages.len();
    let mut out = Vec::new();
    for (index, message) in input.messages.into_iter().enumerate() {
        for mut chunk in parsing::chunks(&message.content, "text")? {
            chunk.locator["parser"] = json!(MESSAGE_PARSER);
            chunk.locator["source_path"] = json!(format!("/messages/{index}/content"));
            chunk.locator["byte_basis"] = json!("message_content_utf8");
            chunk.locator["message_index"] = json!(index);
            chunk.locator["message_count"] = json!(count);
            chunk.locator["role"] = json!(message.role);
            chunk.locator["timestamp"] = json!(message.timestamp);
            chunk.locator["session_id"] = json!(input.session_id);
            out.push(chunk);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> AddInput {
        AddInput {
            request_id: "r".into(),
            user_id: "run:甲".into(),
            session_id: "s".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: " 中文🙂 English\n".repeat(500),
                    timestamp: Some(1720000000000),
                },
                Message {
                    role: Role::Assistant,
                    content: "回答 café é".into(),
                    timestamp: None,
                },
            ],
        }
    }
    #[test]
    fn aml_message_boundaries_and_original_utf8_ranges() {
        let input = input();
        let source = serde_json::to_string(&input).unwrap();
        let chunks = chunks(&source).unwrap();
        assert!(chunks.len() > 2);
        for (index, message) in input.messages.iter().enumerate() {
            let parts: Vec<_> = chunks
                .iter()
                .filter(|c| c.locator["message_index"] == index)
                .collect();
            assert_eq!(
                parts.iter().map(|c| c.content.as_str()).collect::<String>(),
                message.content
            );
            for part in parts {
                let start = part.locator["byte_start"].as_u64().unwrap() as usize;
                let end = part.locator["byte_end"].as_u64().unwrap() as usize;
                assert_eq!(part.content, message.content[start..end]);
                assert_eq!(
                    part.locator["source_path"],
                    format!("/messages/{index}/content")
                );
                assert_eq!(part.locator["timestamp"], json!(message.timestamp));
                assert_eq!(part.locator["role"], json!(message.role));
                assert_eq!(part.locator["message_count"], 2);
            }
        }
    }
    #[test]
    fn aml_invalid_payloads_and_limits_fail_at_parser_boundary() {
        for invalid in [b"{private diagnostic".as_slice(), b"[]", b"null"] {
            assert!(matches!(parse_add(invalid), Err(AppError::Invalid(_))));
        }
        let mut input = input();
        for messages in [
            vec![],
            vec![input.messages[0].clone(); MAX_MESSAGES + 1],
            vec![Message {
                role: Role::User,
                content: "a".repeat(parsing::MAX_TEXT + 1),
                timestamp: None,
            }],
            vec![
                Message {
                    role: Role::User,
                    content: "a".repeat(parsing::MAX_TEXT / 2 + 1),
                    timestamp: None
                };
                2
            ],
        ] {
            input.messages = messages;
            assert!(chunks(&serde_json::to_string(&input).unwrap()).is_err());
        }
        input.messages = vec![Message {
            role: Role::User,
            content: "ok".into(),
            timestamp: None,
        }];
        input.request_id = "x".repeat(MAX_ID_BYTES + 1);
        assert!(input.validate().is_err());
        assert!(parse_add(&vec![b' '; parsing::MAX_FILE + 1]).is_err());
        for content in [
            json!([{"type":"image_url","image_url":"private"}]),
            json!("a\0b"),
            json!(" "),
        ] {
            let payload = json!({"request_id":"r","user_id":"u","session_id":"s","messages":[{"role":"user","content":content}]});
            assert!(parse_add(&serde_json::to_vec(&payload).unwrap()).is_err());
        }
        for top_k in [0, 101] {
            assert!(
                parse_search(
                    &serde_json::to_vec(&json!({"query":"x","user_id":"u","top_k":top_k})).unwrap()
                )
                .is_err()
            );
        }
        assert!(
            parse_search(
                &serde_json::to_vec(&json!({"query":"x".repeat(4001),"user_id":"u","top_k":100}))
                    .unwrap()
            )
            .is_err()
        );
    }
}
