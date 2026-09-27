//! Authenticated host access for CLI/MCP; never opens the host's database files.
use crate::error::{AppError, Result};
use serde_json::Value;
#[derive(Clone)]
pub struct HostClient {
    base: reqwest::Url,
    token: String,
    http: reqwest::Client,
}
impl HostClient {
    pub fn new(base: &str, token: String) -> Result<Self> {
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
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(anyhow::Error::from)?;
        Ok(Self { base, token, http })
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
