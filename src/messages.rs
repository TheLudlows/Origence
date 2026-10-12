//! Protocol-independent message batches and immutable message-local provenance.
//!
//! Labels describe source messages, never authority. Input order is authoritative;
//! optional timestamps are signed Unix milliseconds and are never synthesized.
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
pub const MAX_ROLE_BYTES: usize = 64;
pub const MESSAGE_PARSER: &str = "message-batch-ranges-v1";
pub const PARSER_VERSION: u32 = 1;
/// Version 1 uses contiguous, non-overlapping, at-most-2400-byte UTF-8 chunks.
pub const CHUNKING_VERSION: u32 = 1;
pub const SOURCE_KIND: &str = "message_batch_v1";
pub const INGEST_OPERATION: &str = "message_batch.ingest";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageBatch {
    pub batch_id: String,
    pub session_id: String,
    pub messages: Vec<SourceMessage>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

/// Immutable input to the original job, also covered by the command fingerprint.
/// New ingestion currently supports only raw text plus its embedding profile.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AcceptedMessageSpec {
    pub parser: String,
    pub parser_version: u32,
    pub chunking_version: u32,
    pub raw_only: bool,
    pub embedding_profile: String,
}
impl AcceptedMessageSpec {
    pub fn raw_only(embedding_profile: impl Into<String>) -> Self {
        Self {
            parser: MESSAGE_PARSER.into(),
            parser_version: PARSER_VERSION,
            chunking_version: CHUNKING_VERSION,
            raw_only: true,
            embedding_profile: embedding_profile.into(),
        }
    }

    /// Bound untrusted specifications before hashing or storing them. Supported
    /// versions are checked after command lookup so changed specs conflict on replay.
    pub fn validate_bounds(&self) -> Result<()> {
        for value in [&self.parser, &self.embedding_profile] {
            if value.trim().is_empty() || value.len() > MAX_ID_BYTES || value.contains('\0') {
                return Err(AppError::Invalid("invalid message processing specification".into()));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_bounds()?;
        if self.parser != MESSAGE_PARSER
            || self.parser_version != PARSER_VERSION
            || self.chunking_version != CHUNKING_VERSION
            || !self.raw_only
        {
            return Err(AppError::Invalid("unsupported message processing specification".into()));
        }
        Ok(())
    }
}

/// Durable references, not a cached claim of success. Observe publication again
/// whenever a caller needs to know whether this exact accepted batch is retrievable.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IngestReceipt {
    pub batch_id: String,
    pub session_id: String,
    pub asset_id: Uuid,
    pub source_event_id: Uuid,
    pub job_id: Uuid,
    pub target_version: i32,
    pub accepted_spec: AcceptedMessageSpec,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    Pending,
    Retrievable,
    Failed,
    CancelledOrSuperseded,
    Invisible,
}

pub fn validate_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || value.contains('\0') {
        return Err(AppError::Invalid("message IDs must contain 1..4096 UTF-8 bytes without NUL".into()));
    }
    Ok(())
}
impl MessageBatch {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.batch_id)?;
        validate_id(&self.session_id)?;
        if self.messages.is_empty() || self.messages.len() > MAX_MESSAGES {
            return Err(AppError::Invalid("message batch requires 1..256 messages".into()));
        }
        let mut total = 0usize;
        for message in &self.messages {
            if message.role.trim().is_empty()
                || message.role.len() > MAX_ROLE_BYTES
                || message.role.chars().any(char::is_control)
            {
                return Err(AppError::Invalid("message labels require 1..64 UTF-8 bytes without controls".into()));
            }
            parsing::validate_text(&message.content)?;
            total = total.saturating_add(message.content.len());
            if total > parsing::MAX_TEXT {
                return Err(AppError::Invalid("message batch text exceeds 1 MB".into()));
            }
            // Every i64 is a valid signed Unix-millisecond value. Serde rejects
            // fractional/out-of-range values; no date-range normalization occurs.
        }
        Ok(())
    }
}

pub fn parse_batch(bytes: &[u8]) -> Result<MessageBatch> {
    if bytes.len() > parsing::MAX_FILE {
        return Err(AppError::Invalid("message batch source too large".into()));
    }
    let batch: MessageBatch = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Invalid("invalid message batch source".into()))?;
    batch.validate()?;
    Ok(batch)
}

/// CPU parser; callers on an async executor must run this outside transactions
/// on spawn_blocking. Ranges address decoded original message UTF-8, not JSON.
pub fn chunks(source: &str, spec: &AcceptedMessageSpec, source_event_id: Uuid) -> Result<Vec<Chunk>> {
    spec.validate()?;
    let batch = parse_batch(source.as_bytes())?;
    let count = batch.messages.len();
    let mut out = Vec::new();
    for (index, message) in batch.messages.into_iter().enumerate() {
        for mut chunk in parsing::chunks(&message.content, "text")? {
            chunk.locator["parser"] = json!(spec.parser);
            chunk.locator["parser_version"] = json!(spec.parser_version);
            chunk.locator["chunking_version"] = json!(spec.chunking_version);
            chunk.locator["source_event_id"] = json!(source_event_id);
            chunk.locator["external_batch_id"] = json!(batch.batch_id);
            chunk.locator["source_path"] = json!(format!("/messages/{index}/content"));
            chunk.locator["byte_basis"] = json!("message_content_utf8");
            chunk.locator["message_index"] = json!(index);
            chunk.locator["message_count"] = json!(count);
            chunk.locator["role"] = json!(message.role);
            chunk.locator["timestamp"] = json!(message.timestamp);
            chunk.locator["session_id"] = json!(batch.session_id);
            out.push(chunk);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> MessageBatch {
        MessageBatch {
            batch_id: "external:批次:1".into(),
            session_id: "same-session".into(),
            messages: vec![
                SourceMessage { role: "system".into(), content: "Atlas 中文🙂 café é\n".repeat(250), timestamp: Some(i64::MAX) },
                SourceMessage { role: "tool".into(), content: "Atlas first tool response".into(), timestamp: Some(-1) },
                SourceMessage { role: "agent: Planner".into(), content: "Atlas missing timestamp".into(), timestamp: None },
            ],
        }
    }

    #[test]
    fn generic_labels_order_timestamps_and_exact_message_ranges() {
        let batch = batch();
        let source = serde_json::to_string(&batch).unwrap();
        let source_event = Uuid::from_u128(1);
        let chunks = chunks(&source, &AcceptedMessageSpec::raw_only("fixture"), source_event).unwrap();
        assert!(chunks.len() > 3);
        let indices: Vec<_> = chunks.iter().map(|chunk| chunk.locator["message_index"].as_u64().unwrap()).collect();
        assert!(indices.windows(2).all(|pair| pair[0] <= pair[1]));
        for (index, message) in batch.messages.iter().enumerate() {
            let parts: Vec<_> = chunks.iter().filter(|chunk| chunk.locator["message_index"] == index).collect();
            assert_eq!(parts.iter().map(|chunk| chunk.content.as_str()).collect::<String>(), message.content);
            for part in parts {
                let start = part.locator["byte_start"].as_u64().unwrap() as usize;
                let end = part.locator["byte_end"].as_u64().unwrap() as usize;
                assert!(end - start <= 2400);
                assert_eq!(part.content, message.content[start..end]);
                assert_eq!(part.locator["role"], message.role);
                assert_eq!(part.locator["timestamp"], json!(message.timestamp));
                assert_eq!(part.locator["source_event_id"], json!(source_event));
                assert_eq!(part.locator["external_batch_id"], batch.batch_id);
                assert_eq!(part.locator["session_id"], batch.session_id);
                assert_eq!(part.locator["message_count"], 3);
                assert_eq!(part.locator["source_path"], format!("/messages/{index}/content"));
            }
        }
    }

    #[test]
    fn resource_and_json_time_validation_is_shared_by_parser() {
        let batch = batch();
        for label in ["".into(), " ".into(), "x".repeat(MAX_ROLE_BYTES + 1), "tool\n".into(), "tool\0".into()] {
            let mut invalid = batch.clone();
            invalid.messages[0].role = label;
            assert!(parse_batch(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        for timestamp in [json!(1.5), json!("1"), json!(18446744073709551615u64)] {
            let mut invalid = json!(batch);
            invalid["messages"][0]["timestamp"] = timestamp;
            assert!(parse_batch(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        for messages in [vec![], vec![batch.messages[1].clone(); MAX_MESSAGES + 1], vec![SourceMessage { content: "x".repeat(parsing::MAX_TEXT + 1), ..batch.messages[1].clone() }]] {
            let invalid = MessageBatch { messages, ..batch.clone() };
            assert!(parse_batch(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        assert!(parse_batch(&vec![b' '; parsing::MAX_FILE + 1]).is_err());
        let source = serde_json::to_string(&batch).unwrap();
        let mut spec = AcceptedMessageSpec::raw_only("fixture");
        for version in [0, 2] {
            spec.chunking_version = version;
            assert!(chunks(&source, &spec, Uuid::nil()).is_err());
        }
    }
}
