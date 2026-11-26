use crate::support::open_service;
use origence::{
    api::{McpHttpConfig, router_with_mcp},
    models::Models,
    service::Service,
    storage::{RelationalStore, Scope},
};
use reqwest::{RequestBuilder, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

const PROTOCOL: &str = "2025-11-25";

struct Peer {
    url: String,
    client: reqwest::Client,
}

impl Peer {
    async fn start(service: Service, config: McpHttpConfig) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router_with_mcp(service, config))
                .await
                .unwrap()
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();
        Self { url, client }
    }

    fn post(&self, message: Value) -> RequestBuilder {
        self.client
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL)
            .json(&message)
    }

    async fn call(
        &self,
        token: &str,
        workspace: Option<Uuid>,
        tool: &str,
        args: Value,
    ) -> Result<Value, Value> {
        let mut request = self
            .post(rpc("tools/call", json!({"name":tool,"arguments":args})))
            .bearer_auth(token);
        if let Some(id) = workspace {
            request = request.header("X-Origence-Workspace", id.to_string());
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 200);
        let reply: Value = response.json().await.unwrap();
        if let Some(error) = reply.get("error") {
            return Err(error.clone());
        }
        assert_ne!(reply["result"]["isError"], true);
        Ok(serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap())
    }
}

fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}

async fn workspace(service: &Service, tenant_id: Uuid) -> Scope {
    let workspace_id = Uuid::new_v4();
    service
        .engine
        .relational()
        .create_workspace(tenant_id, workspace_id, "MCP fixture")
        .await
        .unwrap();
    Scope {
        tenant_id,
        workspace_id,
    }
}

#[tokio::test]
async fn streamable_http_negotiates_and_checks_auth_host_origin_method_and_size() {
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Models::disabled())
        .await
        .unwrap();
    let scope = workspace(&service, Uuid::new_v4()).await;
    let key = service
        .engine
        .relational()
        .issue_key(scope, "reader")
        .await
        .unwrap();
    let peer = Peer::start(
        service,
        McpHttpConfig::with_public_origin("https://mcp.example.test:8443").unwrap(),
    )
    .await;
    assert!(McpHttpConfig::with_public_origin("http://mcp.example.test").is_err());
    assert!(McpHttpConfig::with_public_origin("https://mcp.example.test/mcp").is_err());
    let initialize = rpc(
        "initialize",
        json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}),
    );
    let missing = peer.post(initialize.clone()).send().await.unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(missing.headers()["www-authenticate"], "Bearer");
    let request = || peer.post(initialize.clone()).bearer_auth(&key.token);
    assert_eq!(
        request()
            .header("Host", "unexpected.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request()
            .header("Origin", "https://unexpected.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request()
            .header("X-Origence-Workspace", Uuid::new_v4().to_string())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let init = request()
        .header("Host", "mcp.example.test:8443")
        .header("Origin", "https://mcp.example.test:8443")
        .send()
        .await
        .unwrap();
    assert_eq!(init.status(), 200);
    assert!(!init.headers().contains_key("mcp-session-id"));
    assert_eq!(init.headers()["cache-control"], "no-store");
    assert_eq!(
        init.json::<Value>().await.unwrap()["result"]["protocolVersion"],
        PROTOCOL
    );
    let ack = peer
        .post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .bearer_auth(&key.token)
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 202);
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let response = peer.client.request(method, &peer.url).send().await.unwrap();
        assert_eq!(response.status(), 405);
        assert_eq!(response.headers()["allow"], "POST");
    }
    let large = request()
        .body(vec![b' '; 1024 * 1024 + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(large.status(), 413);
    let tools: Value = peer
        .post(rpc("tools/list", json!({})))
        .bearer_auth(&key.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for tool in [
        "context_session",
        "context_search",
        "context_get",
        "context_run_prepare",
    ] {
        assert!(
            tools["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == tool)
        );
    }
}

#[tokio::test]
async fn streamable_http_reuses_scoped_tools_and_rechecks_live_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Models::disabled())
        .await
        .unwrap();
    let a = workspace(&service, Uuid::new_v4()).await;
    let b = workspace(&service, a.tenant_id).await;
    let store = service.engine.relational();
    let writer = store.issue_key(a, "writer").await.unwrap();
    let a_admin = store.issue_key(a, "admin").await.unwrap();
    let b_admin = store.issue_key(b, "admin").await.unwrap();
    let reader = store.issue_key(a, "reader").await.unwrap();
    let peer = Peer::start(service.clone(), McpHttpConfig::default()).await;
    let auth = service.auth(&writer.token).await.unwrap();
    let admin_auth = service.auth(&a_admin.token).await.unwrap();
    let receipt = service
        .knowledge(
            &admin_auth,
            "mcp-http-fixture",
            serde_json::from_value(
                json!({"title":"Atlas", "content":"Atlas evidence", "format":"text"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    while origence::worker::process_next(&service).await.unwrap() {}
    let asset = &receipt["asset_id"];
    let b_admin_auth = service.auth(&b_admin.token).await.unwrap();
    service
        .set_workspace_key_grant(&b_admin_auth, writer.key_id, Some("reader"))
        .await
        .unwrap();
    let token = &writer.token;
    let session = peer
        .call(token, None, "context_session", json!({}))
        .await
        .unwrap();
    assert_eq!(session["workspace_id"], a.workspace_id.to_string());
    let other_session = peer
        .call(token, Some(b.workspace_id), "context_session", json!({}))
        .await
        .unwrap();
    assert_eq!(other_session["workspace_id"], b.workspace_id.to_string());
    assert_eq!(session["key_id"], other_session["key_id"]);
    assert_ne!(session["session_id"], other_session["session_id"]);

    let hits = peer
        .call(
            &reader.token,
            None,
            "context_search",
            json!({"query":"Atlas","mode":"keyword"}),
        )
        .await
        .unwrap();
    assert_eq!(hits["hits"][0]["asset_id"], *asset);
    assert_eq!(
        peer.call(
            &reader.token,
            None,
            "context_get",
            json!({"asset_id":asset,"version":1})
        )
        .await
        .unwrap()["content"],
        "Atlas evidence"
    );
    assert!(
        peer.call(
            token,
            Some(b.workspace_id),
            "context_get",
            json!({"asset_id":asset,"version":1})
        )
        .await
        .is_err()
    );

    let run = Uuid::new_v4();
    let create = json!({"run_id":run,"summary":"Private Run"});
    assert!(
        peer.call(
            token,
            Some(b.workspace_id),
            "context_run_create",
            create.clone()
        )
        .await
        .is_err()
    );
    peer.call(token, None, "context_run_create", create)
        .await
        .unwrap();
    let event_id = Uuid::new_v4();
    let context = json!({"query":"Atlas","mode":"keyword"});
    let prepare = peer
        .call(
            token,
            None,
            "context_run_prepare",
            json!({"run_id":run,"event_id":event_id,"context":context}),
        )
        .await
        .unwrap();
    let replay = service
        .prepare_run_context(
            &auth,
            run,
            event_id,
            serde_json::from_value(context).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay, prepare);
    assert!(
        peer.call(
            &reader.token,
            None,
            "context_run_get",
            json!({"run_id":run})
        )
        .await
        .is_err()
    );

    let turn = Uuid::new_v4();
    peer.call(token, None, "context_turn_archive", json!({"turn":{"turn_id":turn,"question":"Synthetic question","answer":"Synthetic answer","used_evidence":[]}})).await.unwrap();
    assert_eq!(
        peer.call(token, None, "context_turn_get", json!({"turn_id":turn}))
            .await
            .unwrap()["answer"],
        "Synthetic answer"
    );
    assert!(
        peer.call(
            token,
            Some(b.workspace_id),
            "context_turn_get",
            json!({"turn_id":turn})
        )
        .await
        .is_err()
    );
    service
        .revoke_key(&service.auth(&a_admin.token).await.unwrap(), writer.key_id)
        .await
        .unwrap();
    assert_eq!(
        peer.post(rpc("tools/list", json!({})))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}
