//! Native, request-scoped Streamable HTTP MCP transport.
use super::http::auth;
use crate::{error::AppError, mcp::ContextMcp, service::Service};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use std::sync::Arc;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// By default the endpoint accepts loopback hosts and requests without an Origin.
#[derive(Clone)]
pub struct McpHttpConfig {
    transport: StreamableHttpServerConfig,
    origin: Option<HeaderValue>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            transport: StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true),
            origin: None,
        }
    }
}

impl McpHttpConfig {
    /// Allow a known public HTTPS origin when the host is behind a TLS proxy.
    pub fn with_public_origin(origin: &str) -> crate::error::Result<Self> {
        let invalid = || AppError::Invalid("MCP public origin must be an HTTPS origin".into());
        let url = reqwest::Url::parse(origin).map_err(|_| invalid())?;
        if url.scheme() != "https"
            || url.path() != "/"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let host = url.host_str().ok_or_else(invalid)?;
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        let mut config = Self::default();
        config.transport.allowed_hosts.push(authority);
        config.origin = Some(
            HeaderValue::from_str(&url.origin().ascii_serialization()).map_err(|_| invalid())?,
        );
        Ok(config)
    }
}

#[derive(Clone)]
struct Endpoint {
    service: Service,
    config: McpHttpConfig,
    sessions: Arc<NeverSessionManager>,
}

pub(super) fn router(service: Service, config: McpHttpConfig) -> Router {
    Router::new()
        .route("/mcp", any(handle))
        .with_state(Endpoint {
            service,
            config,
            sessions: Arc::new(NeverSessionManager::default()),
        })
}

async fn handle(State(endpoint): State<Endpoint>, request: Request) -> Response {
    let mut response = dispatch(endpoint, request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if response.status() == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

async fn dispatch(endpoint: Endpoint, mut request: Request) -> Response {
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    if origins
        .next()
        .is_some_and(|origin| Some(origin) != endpoint.config.origin.as_ref())
        || origins.next().is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    if request.method() != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response();
    }
    if request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        > 1
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // Claude Code expands the optional plugin setting to an empty header when
    // no workspace is selected. Leave duplicate and malformed selections to auth.
    let workspace_header = "x-origence-workspace";
    let mut selections = request.headers().get_all(workspace_header).iter();
    if selections.next().is_some_and(HeaderValue::is_empty) && selections.next().is_none() {
        request.headers_mut().remove(workspace_header);
    }
    let identity = match auth(&endpoint.service, request.headers()).await {
        Ok(identity) => identity,
        Err(error) => return error.into_response(),
    };
    // The SDK receives request parts. Keep the bearer token out of its extensions.
    let (mut parts, body) = request.into_parts();
    parts.headers.remove(header::AUTHORIZATION);
    let Ok(body) = to_bytes(body, MAX_REQUEST_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let service = StreamableHttpService::new(
        move || Ok(ContextMcp::new(endpoint.service.clone(), identity.clone())),
        endpoint.sessions,
        endpoint.config.transport,
    );
    service
        .handle(Request::from_parts(parts, Body::from(body)))
        .await
        .into_response()
}
