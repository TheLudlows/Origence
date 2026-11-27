//! Adapt Origence's publication/result conventions to the typed in-process SDK.
use super::*;
use crate::retrieval_diagnostics::{QueryObservation, error_code};
use anyseek::sdk::{CreateIndexRequest, LocalEngine, LocalError, LocalErrorKind, OperationId};
use serde::de::DeserializeOwned;
use std::io::{self, Write};

#[derive(Deserialize)]
struct Create {
    operation_id: OperationId,
    collection_id: String,
    config: CreateIndexRequest,
    embedding_space_id: Option<String>,
}

struct LimitedOutput {
    limit: usize,
    len: usize,
    bytes: Option<Vec<u8>>,
}

impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.len {
            return Err(io::Error::other(
                "AnySeek payload exceeds its encoded limit",
            ));
        }
        self.len += bytes.len();
        if let Some(buffer) = &mut self.bytes {
            buffer.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn decode_request<T: DeserializeOwned + Send + 'static>(body: Value) -> StorageResult<T> {
    tokio::task::spawn_blocking(move || {
        serde_json::to_writer(
            LimitedOutput {
                limit: MAX_REQUEST,
                len: 0,
                bytes: None,
            },
            &body,
        )
        .map_err(|_| invalid("AnySeek request exceeds 2 MiB"))?;
        serde_json::from_value(body).map_err(|_| inconsistent())
    })
    .await
    .map_err(|_| unavailable())?
}

async fn result_value(value: impl Serialize + Send + 'static) -> StorageResult<Value> {
    // Use the same numeric representation as the HTTP result before hashing
    // prepared bundles. A direct to_value conversion widens f32 scores.
    tokio::task::spawn_blocking(move || {
        let mut bounded = LimitedOutput {
            limit: MAX_RESPONSE,
            len: 0,
            bytes: Some(Vec::new()),
        };
        serde_json::to_writer(&mut bounded, &value).map_err(|_| unavailable())?;
        serde_json::from_slice(&bounded.bytes.unwrap()).map_err(|_| unavailable())
    })
    .await
    .map_err(|_| unavailable())?
}

fn rejected(
    error: LocalError,
    observation: &mut Option<QueryObservation>,
) -> StorageResult<(StatusCode, Value)> {
    if let Some(entry) = observation {
        entry.outcome = "sdk_error".into();
        entry.backend_code = error_code(error.code());
    }
    let status = match error.kind() {
        LocalErrorKind::NotFound => StatusCode::NOT_FOUND,
        LocalErrorKind::Rejected => StatusCode::BAD_REQUEST,
        LocalErrorKind::Conflict => StatusCode::CONFLICT,
        LocalErrorKind::Expired => StatusCode::GONE,
        LocalErrorKind::Unavailable => return Err(transient()),
    };
    Ok((status, Value::Null))
}

pub(super) async fn request(
    engine: &LocalEngine,
    scope: Option<Scope>,
    collection: Option<&str>,
    request: Request,
    observation: &mut Option<QueryObservation>,
) -> StorageResult<(StatusCode, Value)> {
    if matches!(request, Request::Live) {
        return match engine.version() {
            Ok(version) => Ok((StatusCode::OK, json!(version))),
            Err(error) => rejected(error, observation),
        };
    }
    let scope = scope.ok_or_else(inconsistent)?;
    let tenant = scope.tenant_id.to_string();
    let collection = collection.ok_or_else(inconsistent)?;
    let group_version = matches!(request, Request::GroupVersion(_));
    let result = match request {
        Request::Info => engine.collection_info(&tenant, collection).await,
        Request::Create(body) => {
            let args: Create = decode_request(body).await?;
            if args.collection_id != collection {
                return Err(inconsistent());
            }
            engine
                .create_collection(
                    &tenant,
                    collection,
                    args.operation_id,
                    args.config,
                    args.embedding_space_id,
                )
                .await
                .map(|()| Value::Null)
        }
        Request::Query(body) => {
            let query = decode_request(body).await?;
            let result = match engine.query(&tenant, collection, query).await {
                Ok(result) => result,
                Err(error) => return rejected(error, observation),
            };
            if let Some(entry) = observation {
                entry.backend_request_id = Some(result.request_id().to_owned());
                entry.outcome = "received".into();
            }
            let value = result_value(result).await?;
            AnySeekFullTextStore::observe_result(&value, observation)?;
            return Ok((StatusCode::OK, value));
        }
        Request::GroupVersion(group) => engine
            .group_version(&tenant, collection, &group)
            .await
            .map(|version| {
                version.map_or(
                    Value::Null,
                    |version| json!({"group_id":group,"group_version":version}),
                )
            }),
        Request::ReplaceGroup(body) => {
            let group: anyseek::sdk::GroupReplace = decode_request(body).await?;
            let operation_id = group.operation_id.clone();
            engine
                .replace_group(&tenant, collection, group)
                .await
                .map(|receipt| json!({"atomic":true,"operation_id":operation_id,"receipt":receipt}))
        }
        // Only HTTP uses a namespace listing to distinguish hidden IDs from
        // authorization errors. The trusted SDK reports a missing ID directly.
        Request::List | Request::Live => return Err(inconsistent()),
    };
    match result {
        Ok(Value::Null) if group_version => Ok((StatusCode::NOT_FOUND, Value::Null)),
        Ok(value) => Ok((StatusCode::OK, value)),
        Err(error) => rejected(error, observation),
    }
}
