use super::*;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind},
    application::{FileIndexRootConfig, RuntimeConfiguration, research::RESEARCH_TEST_LOCK},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

async fn fixture() -> (
    PathBuf,
    RelayKnowledgeService,
    Value,
    Value,
    Arc<dyn crate::storage::KnowledgeStore>,
) {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!("research-web-{:x}", Sha256::digest(nonce)));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let raw = b"Source statement\r\n";
    tokio::fs::write(root.join("raw.txt"), raw).await.unwrap();
    let artifact = json!({"path_base":"repository","path":"raw.txt","sha256":format!("{:x}",Sha256::digest(raw))});
    let catalog = json!({"schema_version":1,"adapter":"relay-capture-v1","sources":[{"id":"source","url":"https://example.org","raw":artifact}]});
    tokio::fs::write(
        root.join("catalog.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .await
    .unwrap();
    let bundle = json!({"schema_version":1,"id":"web-study","source_scope":"research","graph":{"nodes":[{"id":"a","kind":"concept","label":"A"},{"id":"b","kind":"concept","label":"B"}],"edges":[{"source":"a","target":"b","relation":"supports","evidence":["pin"],"qualifiers":{"author":"unreviewed"}}]},"evidence":[{"id":"pin","source_scope":"research","artifact":artifact,"interpretation":"user_scope_confirmation"}]});
    tokio::fs::write(
        root.join("bundle.json"),
        serde_json::to_vec(&bundle).unwrap(),
    )
    .await
    .unwrap();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", root.to_str().unwrap()),
            ("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap()),
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "local"),
        ],
    )
    .unwrap();
    let mut runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    runtime.file_index.roots = vec![FileIndexRootConfig::new("research", root.clone())];
    let store: Arc<dyn crate::storage::KnowledgeStore> =
        Arc::new(SqliteGraphStore::open_in_memory().unwrap());
    let service = RelayKnowledgeService::with_store(runtime, store.clone());
    let target = json!({"kind":"configured","path":root,"source_scope":"research"});
    (root, service, target, bundle, store)
}

async fn execute(service: &RelayKnowledgeService, payload: Value, status: StatusCode) -> Value {
    let body = json!({"snapshot":{"name":"Research workflow","command":"shared application operation","payload":payload}});
    let response = super::super::router(service.clone(), crate::net::http::DEFAULT_MAX_BODY_BYTES)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/web/operations/execute")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let actual = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(actual, status, "{value}");
    value
}

#[tokio::test]
async fn web_archive_bundle_and_map_operations_share_the_cli_services() {
    let _guard = RESEARCH_TEST_LOCK.lock().await;
    let (root, service, target, bundle, store) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Web);
    let audit = execute(
        &service,
        json!({"operation":"sources.audit","target":target,"input":"catalog.json"}),
        StatusCode::OK,
    )
    .await;
    assert_eq!(audit["result"]["integrity_valid"], true);
    let status=execute(&service,json!({"operation":"research.status","target":target,"delivery":"archive","catalog":"catalog.json"}),StatusCode::OK).await;
    assert_eq!(status["result"]["map"]["valid"], false);
    assert_eq!(status["result"]["readiness"], "ready_for_review");
    assert_eq!(status["result"]["repository_index"]["state"], "not_indexed");
    let validated=execute(&service,json!({"operation":"evidence.validate","target":target,"input":"bundle.json","source_scope":"research"}),StatusCode::OK).await;
    assert_eq!(validated["result"]["valid"], true);
    let view=execute(&service,json!({"operation":"evidence.view","target":target,"input":"bundle.json","source_scope":"research","focus":"a"}),StatusCode::OK).await;
    assert_eq!(view["result"]["view"]["graph"], bundle["graph"]);
    let impact=execute(&service,json!({"operation":"evidence.impact","target":target,"input":"bundle.json","source_scope":"research","node":"a","label":"Clarified"}),StatusCode::OK).await;
    assert_eq!(impact["result"]["revision"]["graph"]["nodes"][0]["id"], "a");
    let imported=execute(&service,json!({"operation":"evidence.import","target":target,"input":"bundle.json","source_scope":"research"}),StatusCode::OK).await;
    assert_eq!(imported["result"]["fact_status"], "proposed");
    assert_eq!(
        imported["metadata"]["graph_version"],
        imported["result"]["graph_version"]
    );
    let exported=execute(&service,json!({"operation":"evidence.export","id":"web-study","source_scope":"research","revision":imported["result"]["audit"]["bundle_sha256"]}),StatusCode::OK).await;
    assert_eq!(exported["result"]["bundle"]["graph"], bundle["graph"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&tokio::fs::read(root.join("bundle.json")).await.unwrap())
            .unwrap(),
        bundle
    );
    assert!(
        service
            .list_indexed_code_repositories(context.clone())
            .await
            .unwrap()
            .repositories
            .is_empty()
    );
    assert!(
        store
            .code_repository_at_root(root.to_string_lossy().into_owned())
            .await
            .unwrap()
            .is_none()
    );
    KnowledgeMapService::new(root.clone())
        .init(&context)
        .await
        .unwrap();
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: root.to_string_lossy().into_owned(),
                alias: "research-web".into(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context,
        )
        .await
        .unwrap();
    let transaction = json!({"schema_version":1,"transaction_id":"web-batch","operations":[{"op":"add","id":"raw-source","topic":"research","kind":"file","uri":"raw.txt"}]});
    let target = json!({"kind":"repository","alias":"research-web"});
    let plan = execute(
        &service,
        json!({"operation":"knowledge.map.plan","target":target,"transaction":transaction}),
        StatusCode::OK,
    )
    .await;
    let apply=execute(&service,json!({"operation":"knowledge.map.apply","target":target,"transaction":plan["result"]["transaction"]}),StatusCode::OK).await;
    assert_eq!(apply["result"]["state"], "applied");
    let replay=execute(&service,json!({"operation":"knowledge.map.apply","target":target,"transaction":plan["result"]["transaction"]}),StatusCode::OK).await;
    assert_eq!(replay["result"]["state"], "already_applied");
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn web_rejects_unconfigured_roots_scope_crossing_traversal_and_unknown_fields() {
    let _guard = RESEARCH_TEST_LOCK.lock().await;
    let (root, service, target, _, _store) = fixture().await;
    for payload in [
        json!({"operation":"sources.audit","target":{"kind":"configured","path":"/unconfigured-research-root","source_scope":"research"},"input":"catalog.json"}),
        json!({"operation":"sources.audit","target":{"kind":"configured","path":"relative","source_scope":"research"},"input":"catalog.json"}),
        json!({"operation":"sources.audit","target":{"kind":"repository","alias":"missing"},"input":"catalog.json"}),
        json!({"operation":"sources.audit","target":target,"input":"../catalog.json"}),
        json!({"operation":"sources.audit","target":target,"input":"catalog.json","root":"/arbitrary"}),
        json!({"operation":"evidence.import","target":target,"input":"bundle.json","source_scope":"foreign"}),
        json!({"operation":"evidence.validate","target":target}),
    ] {
        execute(&service, payload, StatusCode::BAD_REQUEST).await;
    }
    assert!(
        service
            .list_indexed_code_repositories(RequestContext::for_interface(InterfaceKind::Web))
            .await
            .unwrap()
            .repositories
            .is_empty()
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
