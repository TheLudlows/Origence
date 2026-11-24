//! Authenticated host access for CLI/MCP; never opens the host's database files.
use crate::error::{AppError, Result};
use serde_json::Value;
#[derive(Clone)]
pub struct HostClient {
    base: reqwest::Url,
    token: String,
    http: reqwest::Client,
    workspace: Option<uuid::Uuid>,
}
impl HostClient {
    pub fn new(base: &str, token: String) -> Result<Self> {
        Self::with_timeout(
            Self::parse_base(base)?,
            token,
            std::time::Duration::from_secs(60),
        )
    }

    pub(crate) fn parse_base(base: &str) -> Result<reqwest::Url> {
        let base = reqwest::Url::parse(base)
            .map_err(|_| AppError::Invalid("invalid server URL".into()))?;
        let local = matches!(
            base.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
        if (base.scheme() != "https" && !(local && base.scheme() == "http"))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(AppError::Invalid("server URL requires HTTPS (HTTP allowed on loopback) and no credentials/query/fragment".into()));
        }
        Ok(base)
    }

    pub(crate) fn with_timeout(
        base: reqwest::Url,
        token: String,
        timeout: std::time::Duration,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(anyhow::Error::from)?;
        Ok(Self {
            base,
            token,
            http,
            workspace: None,
        })
    }

    /// Bind this client copy to one explicit scope for all subsequent requests.
    pub fn with_workspace(mut self, workspace: uuid::Uuid) -> Self {
        self.workspace = Some(workspace);
        self
    }
    /// Create a private Run. The UUID is the durable retry identity.
    pub async fn create_run(&self, input: &crate::run::RunInput) -> Result<Value> {
        self.call(
            reqwest::Method::POST,
            "/v1/runs",
            Some(&serde_json::json!(input)),
            None,
        )
        .await
    }

    /// Append an explicit claim; never converts reports into server attestations.
    pub async fn append_run_event(
        &self,
        run: uuid::Uuid,
        input: &crate::run::RunEventInput,
    ) -> Result<Value> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/runs/{run}/events"),
            Some(&serde_json::json!(input)),
            None,
        )
        .await
    }

    /// Use the HTTP authority's preparation/replay/failure path unchanged.
    pub async fn prepare_run_context(
        &self,
        run: uuid::Uuid,
        event_id: uuid::Uuid,
        context: &crate::evidence::EvidenceInput,
    ) -> Result<Value> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/runs/{run}/context"),
            Some(&serde_json::json!({"event_id":event_id,"context":context})),
            None,
        )
        .await
    }

    pub async fn run_trace(&self, run: uuid::Uuid, after: i64, limit: usize) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/runs/{run}?after={after}&limit={limit}"),
            None,
            None,
        )
        .await
    }

    pub async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        key: Option<&str>,
    ) -> Result<Value> {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(AppError::Invalid("invalid host API path".into()));
        }
        let url = self.base.join(path).map_err(anyhow::Error::from)?;
        let mut request = self.http.request(method, url).bearer_auth(&self.token);
        if let Some(workspace) = self.workspace {
            request = request.header("X-Origence-Workspace", workspace.to_string());
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| AppError::Unavailable("local host request failed".into()))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(part) = response
            .chunk()
            .await
            .map_err(|_| AppError::Unavailable("host response interrupted".into()))?
        {
            if bytes.len() + part.len() > 8 * 1024 * 1024 {
                return Err(AppError::Unavailable("host response too large".into()));
            }
            bytes.extend_from_slice(&part);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::Unavailable("invalid host response".into()))?;
        if status.is_success() {
            return Ok(value);
        }
        let message = value["error"]["message"]
            .as_str()
            .unwrap_or("host operation failed")
            .to_string();
        Err(match status.as_u16() {
            401 => AppError::Unauthorized,
            403 => AppError::Forbidden,
            404 => AppError::NotFound,
            409 => AppError::Conflict(message),
            422 => AppError::Invalid(message),
            _ => AppError::Unavailable(message),
        })
    }
}
