//! Public Service and HTTP coverage using fresh local stores and synthetic PDFs.
use crate::{pdf_fixture::text_pdf, support::open_service};
use origence::{
    models::Models,
    storage::{RelationalStore, Scope},
    worker,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn run_pdf_source_binds_exact_version_and_rechecks_access() {
    let root = tempfile::tempdir().unwrap();
    let service = open_service(root.path(), Models::disabled()).await.unwrap();
    let store = service.engine.relational();
    let scope = Scope {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
    };
    store
        .create_workspace(scope.tenant_id, scope.workspace_id, "synthetic PDF")
        .await
        .unwrap();
    let owner = store.issue_key(scope, "admin").await.unwrap();
    let other = store.issue_key(scope, "admin").await.unwrap();
    let reader = store.issue_key(scope, "reader").await.unwrap();
    let a = service.auth(&owner.token).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = origence::api::router(service.clone());
    let host = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();

    let v1 = text_pdf(&["cover page", "NeedleUnique historical PDF page"]);
    let v1_file = service
        .upload(&a, "pdf-one", "v1.pdf", "pdf", &v1)
        .await
        .unwrap();
    let v1_asset = service
        .knowledge(
            &a,
            "version-one",
            serde_json::from_value(json!({
                "title":"Versioned PDF","file_id":v1_file["file_id"],"format":"pdf"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    while worker::process_next(&service).await.unwrap() {}

    let run = Uuid::new_v4();
    assert!(
        http.post(format!("{base}/v1/runs"))
            .bearer_auth(&owner.token)
            .json(&json!({"run_id":run,"summary":"source preview"}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let trace_url = format!("{base}/v1/runs/{run}");
    let prepared = http
        .post(format!("{trace_url}/context"))
        .bearer_auth(&owner.token)
        .json(
            &json!({"event_id":Uuid::new_v4(),"context":{"query":"NeedleUnique","mode":"keyword"}}),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(prepared["preparation"]["sequence"], 1);
    let trace = http
        .get(&trace_url)
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let reference = trace["events"][0]["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["locator"]["page"] == 2 && r["reference"]["asset_id"] == v1_asset["asset_id"])
        .unwrap();
    let evidence: Uuid = serde_json::from_value(reference["evidence_ref_id"].clone()).unwrap();

    let v2 = text_pdf(&["replacement cover", "NeedleUnique replacement V2 PDF page"]);
    let v2_file = service
        .upload(&a, "pdf-two", "v2.pdf", "pdf", &v2)
        .await
        .unwrap();
    service.knowledge(&a, "version-two", serde_json::from_value(json!({
        "title":"Versioned PDF","file_id":v2_file["file_id"],"format":"pdf","asset_id":v1_asset["asset_id"],"expected_version":1
    })).unwrap()).await.unwrap();
    while worker::process_next(&service).await.unwrap() {}
    let asset: Uuid = serde_json::from_value(v1_asset["asset_id"].clone()).unwrap();
    assert_eq!(service.get(&a, asset, None).await.unwrap()["version"], 2);

    let preview_url = format!("{trace_url}/pdf-source?sequence=1&evidence_ref_id={evidence}");
    let original_url = format!("{preview_url}&original=true");
    // Public library caller and HTTP/relay-facing response use the same guards.
    let library =
        serde_json::to_value(service.run_pdf_preview(&a, run, 1, evidence).await.unwrap()).unwrap();
    let preview = http
        .get(&preview_url)
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(library, preview);
    assert_eq!(preview["asset_version"], 1);
    assert_eq!(preview["page"], 2);
    let marked = preview["span"]["highlight"].as_str().unwrap();
    assert!(marked.contains("NeedleUnique historical PDF page"));
    assert!(!marked.contains("replacement"));
    let original = http
        .get(&original_url)
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(original.headers()["cache-control"], "no-store");
    assert_eq!(original.headers()["content-disposition"], "attachment");
    assert_eq!(original.bytes().await.unwrap().as_ref(), v1.as_slice());
    for url in [&preview_url, &original_url] {
        assert_eq!(
            http.get(url)
                .bearer_auth(&other.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            http.get(url)
                .bearer_auth(&reader.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let wrong = format!(
        "{trace_url}/pdf-source?sequence=1&evidence_ref_id={}",
        Uuid::new_v4()
    );
    assert_eq!(
        http.get(&wrong)
            .bearer_auth(&owner.token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        http.get(format!("{preview_url}&version=2"))
            .bearer_auth(&owner.token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    // Real repeated-text source: an archived chunk still lets the owner open
    // the page, but a repeated quote is not exposed as a verified preview.
    let repeated_text = "EchoPage ".repeat(550);
    let ambiguous = text_pdf(&[&repeated_text]);
    let file = service
        .upload(&a, "repeat-file", "repeat.pdf", "pdf", &ambiguous)
        .await
        .unwrap();
    let repeated_asset = service
        .knowledge(
            &a,
            "repeat-asset",
            serde_json::from_value(json!({
                "title":"Repeated PDF","file_id":file["file_id"],"format":"pdf"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    while worker::process_next(&service).await.unwrap() {}
    let search = service
        .search(
            &a,
            serde_json::from_value(json!({"query":"EchoPage","mode":"keyword"})).unwrap(),
        )
        .await
        .unwrap();
    let first = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| {
            hit["asset_id"] == repeated_asset["asset_id"] && hit["locator"]["byte_start"] == 0
        })
        .unwrap();
    let reported = http
        .post(format!("{trace_url}/events"))
        .bearer_auth(&owner.token)
        .json(&json!({
            "event_id":Uuid::new_v4(),"reporter":"client","observed_at_ms":2,
            "event":{"kind":"context_reported","references":[{
                "kind":"chunk","id":first["chunk_id"],"chunk_id":first["chunk_id"],
                "asset_id":repeated_asset["asset_id"],"asset_version":1
            }]}
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(reported["sequence"], 2);
    let trace = http
        .get(&trace_url)
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let fallback_id = trace["events"][1]["references"][0]["evidence_ref_id"]
        .as_str()
        .unwrap();
    let fallback = format!("{trace_url}/pdf-source?sequence=2&evidence_ref_id={fallback_id}");
    let fallback_preview = http
        .get(&fallback)
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        fallback_preview,
        json!({"asset_version":1, "page":1, "span":null})
    );
    let bytes = http
        .get(format!("{fallback}&original=true"))
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), ambiguous.as_slice());

    let source: Uuid = serde_json::from_value(v1_asset["source_event_id"].clone()).unwrap();
    service
        .delete(&a, "withdraw-source-v1", source, "event")
        .await
        .unwrap();
    for url in [&preview_url, &original_url] {
        assert_eq!(
            http.get(url)
                .bearer_auth(&owner.token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(service.get(&a, asset, None).await.unwrap()["version"], 2);
    host.abort();
    service.engine.shutdown().await.unwrap();
}
