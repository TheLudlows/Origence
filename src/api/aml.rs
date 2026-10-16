//! AML wire protocol. Scope derivation, policy, waiting, and response conversion
//! stay here; persistence and authorization use the public generic Service API.
pub use super::aml_protocol::{WORKSPACE_NAMESPACE, add_command_key, workspace_id};
use crate::{
    error::{AppError, Result},
    messages::{MessageBatch, SourceMessage},
    parsing,
};
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGES: usize = 256;
pub const MAX_ID_BYTES: usize = 4096;
pub const ADD_WAIT: std::time::Duration = std::time::Duration::from_secs(25 * 60);

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
impl Role {
    fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddResponse {
    pub success: bool,
    pub request_id: String,
    pub user_id: String,
    pub session_id: String,
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

    /// Preserve all batch/session/message bytes while removing AML's routing ID
    /// and translating its two-role protocol into generic bounded labels.
    pub fn into_batch(self) -> MessageBatch {
        MessageBatch {
            batch_id: self.request_id,
            session_id: self.session_id,
            messages: self
                .messages
                .into_iter()
                .map(|message| SourceMessage {
                    role: message.role.label().into(),
                    content: message.content,
                    timestamp: message.timestamp,
                })
                .collect(),
        }
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

#[cfg(feature = "local-storage")]
mod runtime {
    use super::*;
    use crate::{
        messages::{AcceptedMessageSpec, PublicationStatus},
        service::Service,
        types::AuthContext,
    };
    use serde_json::{Value, json};

    /// One acceptance and one read-only wait on the generic durable receipt.
    /// Disconnecting this observer never cancels or duplicates worker-owned work.
    pub async fn add(s: &Service, root: &AuthContext, input: AddInput) -> Result<AddResponse> {
        input.validate()?;
        let profile = s
            .models
            .profile
            .clone()
            .ok_or_else(|| AppError::Unavailable("AML requires an embedding model".into()))?;
        let user_id = input.user_id.clone();
        let target = workspace_id(root.tenant_id, root.workspace_id, &user_id);
        let user = s
            .ensure_owned_workspace(root, target, "AML user workspace")
            .await?;
        let key = add_command_key(&input.request_id);
        let receipt = s
            .submit_message_batch(
                &user,
                &key,
                input.into_batch(),
                AcceptedMessageSpec::raw_only(profile),
            )
            .await?;
        match s.wait_message_batch(&user, &receipt, ADD_WAIT).await? {
            PublicationStatus::Retrievable => Ok(AddResponse {
                success: true,
                request_id: receipt.batch_id,
                user_id,
                session_id: receipt.session_id,
            }),
            PublicationStatus::Pending => Err(AppError::Unavailable(
                "AML Add is still pending; retry with the same request_id".into(),
            )),
            PublicationStatus::Failed => Err(AppError::Unavailable(
                "AML Add failed; its original job requires authorized retry".into(),
            )),
            PublicationStatus::CancelledOrSuperseded => {
                Err(AppError::Conflict("AML Add did not complete".into()))
            }
            PublicationStatus::Invisible => Err(AppError::Conflict(
                "AML evidence is no longer searchable".into(),
            )),
        }
    }

    /// Options remain accepted but unused. This route only returns raw vector
    /// evidence, with no summary, graph, reranker, partial fallback, or answer.
    pub async fn search(s: &Service, root: &AuthContext, input: SearchInput) -> Result<Value> {
        input.validate()?;
        let target = workspace_id(root.tenant_id, root.workspace_id, &input.user_id);
        let Some(user) = s.lookup_owned_workspace(root, target).await? else {
            return Ok(json!({"data":[]}));
        };
        let found = s
            .search(
                &user,
                crate::types::SearchInput {
                    query: input.query,
                    limit: input.top_k,
                    mode: "vector".into(),
                    allow_partial: false,
                    memory_identity: None,
                    components: crate::types::RetrievalComponents {
                        summaries: false,
                        graph: false,
                    },
                },
            )
            .await?;
        let hits: Vec<crate::types::SearchHit> = crate::service::decode(found["hits"].clone())?;
        let data: Vec<Value> = hits
            .into_iter()
            .map(|hit| {
                let content = if hit.locator["parser"] == crate::messages::MESSAGE_PARSER {
                    let metadata = json!({
                        "role":hit.locator["role"],
                        "timestamp":hit.locator["timestamp"],
                        "session_id":hit.locator["session_id"],
                        "external_batch_id":hit.locator["external_batch_id"],
                        "source_event_id":hit.source_event_id,
                        "message_index":hit.locator["message_index"],
                        "message_count":hit.locator["message_count"],
                        "parser":hit.locator["parser"],
                        "parser_version":hit.locator["parser_version"],
                        "chunking_version":hit.locator["chunking_version"],
                        "source_path":hit.locator["source_path"],
                        "byte_basis":hit.locator["byte_basis"],
                        "byte_start":hit.locator["byte_start"],
                        "byte_end":hit.locator["byte_end"]
                    });
                    format!("Source metadata: {metadata}\n{}", hit.content)
                } else {
                    hit.content
                };
                json!({"id":hit.chunk_id,"content":content})
            })
            .collect();
        Ok(json!({"data":data}))
    }
}
#[cfg(feature = "local-storage")]
pub use runtime::{add, search};
