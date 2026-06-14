use super::*;
use axum::{
    body::to_bytes,
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

use crate::{
    api::{IngestEvidenceExtraction, IngestRequest},
    application::RelayKnowledgeService,
    domain::{CodeIndexMode, CodeQueryKind, EvidenceModality, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
};

#[test]
fn rejects_asset_path_traversal() {
    let root = PathBuf::from("/srv/web");

    assert!(sanitized_asset_path(&root, "assets/main.js").is_some());
    assert_eq!(
        sanitized_asset_path(&root, "assets/./main.js"),
        Some(root.join("assets").join("main.js"))
    );
    assert!(sanitized_asset_path(&root, "../secret").is_none());
    assert!(sanitized_asset_path(&root, "/etc/passwd").is_none());
}

#[test]
fn reports_expected_content_types() {
    assert_eq!(
        content_type(Path::new("index.html")),
        "text/html; charset=utf-8"
    );
    assert_eq!(
        content_type(Path::new("assets/main.js")),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(content_type(Path::new("data.json")), "application/json");
    assert_eq!(content_type(Path::new("icon.svg")), "image/svg+xml");
    assert_eq!(content_type(Path::new("module.wasm")), "application/wasm");
    assert_eq!(
        content_type(Path::new("asset.bin")),
        "application/octet-stream"
    );
}

#[tokio::test]
async fn default_router_can_be_constructed() {
    let service = test_service("default-router").await;

    let _router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
}

#[tokio::test]
async fn api_error_response_maps_stable_status_codes() {
    let invalid = api_error_response(ApiError::invalid_argument("bad"));
    let storage = api_error_response(ApiError::storage_unavailable("down"));
    let qos = api_error_response(ApiError::qos_rejected("busy"));
    let timeout = api_error_response(ApiError {
        error_kind: ErrorKind::Timeout,
        message: "slow".to_owned(),
        metadata: None,
    });
    let internal = api_error_response(ApiError {
        error_kind: ErrorKind::Internal,
        message: "broken".to_owned(),
        metadata: None,
    });

    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(storage.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(qos.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(timeout.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(internal.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn serves_project_health_and_service_status_apis() {
    let router = test_router("api").await;

    let project = get_json(router.clone(), "/api/project/status").await;
    let health = get_json(router.clone(), "/api/health").await;
    let service = get_json(router, "/api/service/status").await;

    assert_eq!(project["project_name"], "relay-knowledge");
    assert_eq!(health["healthy"], true);
    assert_eq!(service["service_name"], "relay-knowledge");
}

#[tokio::test]
async fn serves_versioned_control_plane_read_apis() {
    let router = test_router("control-api").await;

    let project = get_json(router.clone(), "/api/v1/control/status").await;
    let health = get_json(router.clone(), "/api/v1/control/health").await;
    let service = get_json(router.clone(), "/api/v1/control/service/status").await;
    let storage = get_json(router, "/api/v1/control/storage/topology").await;

    assert_eq!(project["project_name"], "relay-knowledge");
    assert_eq!(health["healthy"], true);
    assert_eq!(service["service_name"], "relay-knowledge");
    assert_eq!(storage["storage"]["topology"], "single_sqlite");
    assert_eq!(storage["storage"]["missing_shard_count"], 0);
}

#[tokio::test]
async fn serves_model_provider_config_apis() {
    let router = test_router("model-config").await;
    let profiles = get_json(router.clone(), "/api/configs/model/profiles").await;
    assert_eq!(profiles["loaded"], true);
    assert_eq!(profiles["profiles"].as_array().unwrap().len(), 0);

    let save_body = json!({
        "provider": "echo",
        "model": "echo",
        "temperature": 0.2,
        "top_p": 1.0,
        "connect_timeout_seconds": 5.0,
        "is_default": true
    });
    let saved = request_json(
        router.clone(),
        "PUT",
        "/api/configs/model/profiles/web-echo",
        Some(save_body),
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["default_profile"], "web-echo");
    assert_eq!(saved["profiles"][0]["provider"], "echo");

    let fallback = get_json(router.clone(), "/api/configs/model-fallback").await;
    assert_eq!(fallback["policies"].as_array().unwrap().len(), 2);
    let saved_fallback = request_json(
        router.clone(),
        "PUT",
        "/api/configs/model-fallback",
        Some(fallback),
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved_fallback["policies"].as_array().unwrap().len(), 2);

    let catalog = get_json(router.clone(), "/api/configs/model/catalog").await;
    assert_eq!(catalog["ok"], true);
    assert!(!catalog["providers"].as_array().unwrap().is_empty());
    let refreshed = request_json(
        router.clone(),
        "POST",
        "/api/configs/model/catalog:refresh",
        None,
        StatusCode::OK,
    )
    .await;
    assert!(!refreshed["providers"].as_array().unwrap().is_empty());

    let probe = request_json(
        router.clone(),
        "POST",
        "/api/configs/model:probe",
        Some(json!({"profile_name": "web-echo", "timeout_ms": 5})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(probe["ok"], true);
    assert_eq!(probe["provider"], "echo");

    let discovery = request_json(
        router.clone(),
        "POST",
        "/api/configs/model:discover",
        Some(json!({"profile_name": "web-echo", "timeout_ms": 5})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(discovery["models"][0], "echo");

    let deleted = request_json(
        router,
        "DELETE",
        "/api/configs/model/profiles/web-echo",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(deleted["profiles"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn serves_static_assets_and_spa_fallback() {
    let router = test_router("assets").await;

    let index = get_text(router.clone(), "/").await;
    let asset = get_text(router.clone(), "/assets/main.js").await;
    let fallback = get_text(router, "/workspace/graph").await;

    assert!(index.contains("<title>relay-knowledge</title>"));
    assert_eq!(asset, "console.log('relay');");
    assert!(fallback.contains("<title>relay-knowledge</title>"));
}

#[tokio::test]
async fn api_misses_do_not_fall_back_to_index() {
    let response = test_router("api-miss")
        .await
        .oneshot(
            Request::builder()
                .uri("/api/missing")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response_text(response).await, "{\"message\":\"not found\"}");
}

#[tokio::test]
async fn missing_web_assets_report_build_guidance() {
    let root = unique_temp_dir("missing-assets");
    let service = test_service("missing-assets").await;
    let response = router_with_assets(service, root, crate::net::http::DEFAULT_MAX_BODY_BYTES)
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        response_text(response)
            .await
            .contains("web assets are not built")
    );
}

#[tokio::test]
async fn executes_ingest_through_web_operation_endpoint() {
    let service = test_service("execute-ingest").await;
    let payload = execute_json(
        service,
        json!({
            "snapshot": {
                "name": "Ingest evidence",
                "command": "relay-knowledge ingest --source docs --content web",
                "payload": {
                    "operation": "graph.ingest",
                    "source_scope": "docs",
                    "content": "Web operation endpoint commits evidence",
                    "entity_labels": ["Web"]
                }
            }
        }),
        StatusCode::OK,
    )
    .await;

    assert_eq!(payload["operation"], "graph.ingest");
    assert_eq!(payload["name"], "Ingest evidence");
    assert_eq!(payload["result"]["receipt"]["evidence_count"], 1);
}

#[tokio::test]
async fn executes_read_and_index_operations_through_shared_service() {
    let service = test_service("execute-read").await;
    let ingest = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Ingest evidence",
                "command": "relay-knowledge ingest --source docs",
                "payload": {
                    "operation": "graph.ingest",
                    "source_scope": "docs",
                    "content": "Executable Web operations retrieve graph evidence",
                    "entity_labels": ["Web", "Graph"]
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(ingest["result"]["receipt"]["evidence_count"], 1);

    let retrieve = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Retrieve context",
                "command": "relay-knowledge query Web",
                "payload": {
                    "operation": "retrieve.context",
                    "query": "Web operations",
                    "source_scope": "docs",
                    "freshness": "allow-stale",
                    "limit": 5
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(retrieve["operation"], "retrieve.context");
    assert_eq!(retrieve["result"]["results"][0]["source_scope"], "docs");

    let graph = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Inspect graph",
                "command": "relay-knowledge graph inspect",
                "payload": {
                    "operation": "graph.inspect",
                    "source_scope": "docs"
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(graph["result"]["graph"]["evidence_count"], 1);

    let index = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Refresh indexes",
                "command": "relay-knowledge index refresh",
                "payload": {
                    "operation": "index.refresh",
                    "kinds": ["bm25", "semantic", "vector"]
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        index["result"]["indexes"]
            .as_array()
            .expect("indexes")
            .len(),
        3
    );

    let status = execute_json(
        service,
        json!({
            "snapshot": {
                "name": "Service runtime",
                "command": "relay-knowledge service doctor",
                "payload": {
                    "operation": "service.doctor",
                    "allowed_scopes": []
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["result"]["service_name"], "relay-knowledge");
}

#[tokio::test]
async fn executes_provider_worker_proposal_and_audit_operations() {
    let service = test_service("execute-ops").await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-worker".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Worker proposal source evidence".to_owned(),
                    entity_labels: Vec::new(),
                    extraction: Some(IngestEvidenceExtraction {
                        modality: EvidenceModality::TextSpan,
                        source_uri: None,
                        source_hash: None,
                        media_hash: None,
                        extractor: Some("web-test".to_owned()),
                        extractor_version: Some("1".to_owned()),
                        observed_at: None,
                        parent_evidence_id: None,
                        layout_region: None,
                        embedding_model: None,
                        embedding_dimension: None,
                        diagnostic: None,
                    }),
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::for_interface(InterfaceKind::Web),
        )
        .await
        .expect("ingest should queue worker tasks");

    let provider = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Probe embedding provider",
                "command": "relay-knowledge provider probe --format json",
                "payload": {
                    "operation": "provider.embedding.probe",
                    "input": "probe"
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(provider["operation"], "provider.embedding.probe");
    assert_eq!(provider["result"]["ok"], false);
    assert_eq!(
        provider["result"]["error_code"],
        "remote_embedding_not_configured"
    );

    let worker = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Worker run-once",
                "command": "relay-knowledge worker run-once --kind embedding --format json",
                "payload": {
                    "operation": "worker.run-once",
                    "kind": "embedding"
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    let proposal_id = worker["result"]["proposals"][0]["proposal_id"]
        .as_str()
        .expect("proposal id should be present")
        .to_owned();

    let proposals = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Proposal list",
                "command": "relay-knowledge proposal list --state proposed --format json",
                "payload": {
                    "operation": "proposal.list",
                    "state": "proposed",
                    "limit": 10
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        proposals["result"]["proposals"][0]["proposal_id"],
        proposal_id
    );

    let shown = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Proposal show",
                "command": format!("relay-knowledge proposal show {proposal_id} --format json"),
                "payload": {
                    "operation": "proposal.show",
                    "proposal_id": proposal_id
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(shown["result"]["proposal"]["proposal_id"], proposal_id);

    let rejected = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Proposal reject",
                "command": "relay-knowledge proposal reject --by web",
                "payload": {
                    "operation": "proposal.reject",
                    "proposal_id": proposal_id,
                    "actor": "web-reviewer",
                    "reason": "covered by web endpoint test"
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(rejected["result"]["proposal"]["state"], "rejected");

    let audit = execute_json(
        service,
        json!({
            "snapshot": {
                "name": "Audit query",
                "command": "relay-knowledge audit query --operation worker.run_once --format json",
                "payload": {
                    "operation": "audit.query",
                    "filter_operation": "worker.run_once",
                    "limit": 10
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(audit["operation"], "audit.query");
    assert!(
        audit["result"]["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["operation"] == "worker.run_once")
    );
}

#[tokio::test]
async fn web_operation_endpoint_maps_bad_payloads_to_http_errors() {
    let service = test_service("execute-errors").await;
    let missing_query = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Retrieve context",
                "command": "relay-knowledge query",
                "payload": {
                    "operation": "retrieve.context",
                    "source_scope": "docs",
                    "freshness": "allow-stale",
                    "limit": 5
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(missing_query["error"], "query is required");

    let bad_kind = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Refresh indexes",
                "command": "relay-knowledge index refresh",
                "payload": {
                    "operation": "index.refresh",
                    "kinds": ["unknown"]
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(bad_kind["error"], "unsupported index kind 'unknown'");

    let missing_repository = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Code status",
                "command": "relay-knowledge repo status missing",
                "payload": {
                    "operation": "code.repo.status",
                    "alias": "missing",
                    "ref": "HEAD"
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        missing_repository["error"],
        "code repository 'missing' is not registered"
    );

    let missing_software_repository = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Software global",
                "command": "relay-knowledge repo software missing",
                "payload": {
                    "operation": "code.repo.software",
                    "alias": "missing",
                    "ref": "HEAD",
                    "kind": "all",
                    "freshness": "allow-stale",
                    "limit": 10
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        missing_software_repository["error"],
        "code repository 'missing' is not registered"
    );

    let bad_repo_set_priority = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Add repository set member",
                "command": "relay-knowledge repo-set add workspace core --ref HEAD --priority invalid",
                "payload": {
                    "operation": "code.repo_set.add",
                    "set_alias": "workspace",
                    "repository_alias": "core",
                    "ref": "HEAD",
                    "priority": "invalid"
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        bad_repo_set_priority["error"],
        "priority must be an integer"
    );

    let bad_repo_set_remove = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Remove repository set member",
                "command": "relay-knowledge repo-set remove workspace core",
                "payload": {
                    "operation": "code.repo_set.remove",
                    "set_alias": "workspace"
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(bad_repo_set_remove["error"], "repository_alias is required");

    let bad_repo_set_async = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Refresh repository set",
                "command": "relay-knowledge repo-set refresh workspace --async",
                "payload": {
                    "operation": "code.repo_set.refresh",
                    "set_alias": "workspace",
                    "async": "true"
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(bad_repo_set_async["error"], "async must be a boolean");

    let bad_worker = execute_json(
        service,
        json!({
            "snapshot": {
                "name": "Worker status",
                "command": "relay-knowledge worker status --kind unknown",
                "payload": {
                    "operation": "worker.status",
                    "kind": "unknown"
                }
            }
        }),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(bad_worker["error"], "unsupported worker kind 'unknown'");
}

#[tokio::test]
async fn web_operation_endpoint_enforces_configured_body_limit() {
    let service = test_service("body-limit").await;
    let router = router(service, 64);
    let request = Request::builder()
        .method("POST")
        .uri("/api/web/operations/execute")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "snapshot": {
                    "name": "Oversized",
                    "command": "relay-knowledge ingest",
                    "payload": {
                        "operation": "graph.ingest",
                        "source_scope": "docs",
                        "content": "this body is intentionally larger than the configured limit",
                        "entity_labels": ["Web"]
                    }
                }
            })
            .to_string(),
        ))
        .expect("request should build");

    let response = router.oneshot(request).await.expect("request should route");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn request_builders_parse_web_payload_variants() {
    let payload = json!({
        "root_path": "/repo",
        "alias": "relay",
        "ref": "main",
        "path_filters": [" src/ ", "tests"],
        "language_filters": [" rust "],
        "query": "handler",
        "kind": "definition",
        "freshness": "wait-until-fresh",
        "limit": 7,
        "base_ref": "main",
        "head_ref": "feature"
    });

    let registration = code_register_request(&payload).expect("registration");
    assert_eq!(registration.alias, "relay");
    assert_eq!(registration.path_filters, ["src/", "tests"]);
    assert_eq!(registration.language_filters, ["rust"]);

    let default_alias_registration =
        code_register_request(&json!({"root_path": "/repo"})).expect("default alias registration");
    assert!(default_alias_registration.alias.is_empty());

    let invalid_alias = code_register_request(&json!({"root_path": "/repo", "alias": 123}))
        .expect_err("numeric alias should be rejected");
    assert_eq!(invalid_alias.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_alias.message, "alias must be a string");

    let selector = code_selector(&payload).expect("selector");
    assert_eq!(selector.repository, "relay");
    assert_eq!(selector.ref_selector, "main");

    let query = code_query_request(&payload).expect("query");
    assert_eq!(query.code_query_kind, CodeQueryKind::Definition);
    assert_eq!(query.freshness_policy, FreshnessPolicy::WaitUntilFresh);

    let file_query = web_files::file_query_request(&json!({
        "query": "design",
        "source_scope": "local-files",
        "root_id": "root-1",
        "freshness": "wait-until-fresh",
        "limit": 7
    }))
    .expect("file query");
    assert_eq!(file_query.source_scope.as_deref(), Some("local-files"));
    assert_eq!(file_query.root_id.as_deref(), Some("root-1"));
    assert_eq!(file_query.freshness_policy, FreshnessPolicy::WaitUntilFresh);

    let default_file_query = web_files::file_query_request(&json!({"query": "design", "limit": 7}))
        .expect("default file query");
    assert_eq!(
        default_file_query.freshness_policy,
        FreshnessPolicy::AllowStale
    );

    let impact = code_impact_request(&payload).expect("impact");
    assert_eq!(impact.base_ref, "main");
    assert_eq!(impact.head_ref, "feature");

    let software_payload = json!({
        "alias": "relay",
        "ref": "main",
        "path_filters": [" src/ ", "tests"],
        "language_filters": [" rust "],
        "kind": "dependencies",
        "freshness": "wait-until-fresh",
        "limit": 7
    });
    let software = code_software_request(&software_payload).expect("software");
    assert_eq!(software.kind, SoftwareGlobalKind::Dependencies);
    assert_eq!(software.freshness_policy, FreshnessPolicy::WaitUntilFresh);

    let index = code_index_request(&payload, CodeIndexMode::Full).expect("index");
    assert!(matches!(index.mode, CodeIndexMode::Full));
}

#[test]
fn payload_validation_reports_actionable_errors() {
    let payload = json!({
        "operation": "retrieve.context",
        "query": " ",
        "freshness": "now",
        "limit": 0,
        "kinds": ["bm25", 42]
    });

    assert_eq!(
        string_field(&payload, "query")
            .expect_err("blank string")
            .message,
        "query is required"
    );
    assert_eq!(
        usize_field(&payload, "limit")
            .expect_err("positive integer")
            .message,
        "limit must be a positive integer"
    );
    assert_eq!(
        string_array_field(&payload, "missing")
            .expect_err("array")
            .message,
        "missing must be an array"
    );
    assert_eq!(
        string_array_field(&payload, "kinds")
            .expect_err("string values")
            .message,
        "kinds contains a non-string value"
    );
    assert_eq!(
        parse_freshness("now").expect_err("freshness").message,
        "unsupported freshness 'now'"
    );
    assert_eq!(
        parse_code_query_kind("sbom").expect("sbom query kind should parse"),
        CodeQueryKind::Sbom
    );
    assert_eq!(
        parse_code_query_kind("impact")
            .expect_err("unsupported web query kind")
            .message,
        "unsupported code query kind 'impact'"
    );
}

async fn test_router(label: &str) -> Router {
    let root = unique_temp_dir(label).join("web");
    std::fs::create_dir_all(root.join("assets")).expect("asset dir should be created");
    std::fs::write(
        root.join("index.html"),
        "<!doctype html><title>relay-knowledge</title>",
    )
    .expect("index asset should be written");
    std::fs::write(root.join("assets").join("main.js"), "console.log('relay');")
        .expect("script asset should be written");

    router_with_assets(
        test_service(label).await,
        root,
        crate::net::http::DEFAULT_MAX_BODY_BYTES,
    )
}

async fn test_service(label: &str) -> RelayKnowledgeService {
    let home = unique_temp_dir(label);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                home.as_path().to_str().expect("utf8 path"),
            ),
        ],
    )
    .expect("environment should parse");

    RelayKnowledgeService::from_environment(&environment)
        .await
        .expect("service should initialize")
}

async fn execute_json(
    service: RelayKnowledgeService,
    body: Value,
    expected_status: StatusCode,
) -> Value {
    let router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/web/operations/execute")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(response.status(), expected_status);

    serde_json::from_str(&response_text(response).await).expect("response should be json")
}

async fn get_json(router: Router, uri: &str) -> Value {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(response.status(), StatusCode::OK);

    serde_json::from_str(&response_text(response).await).expect("response should be json")
}

async fn request_json(
    router: Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    expected_status: StatusCode,
) -> Value {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .oneshot(builder.body(body).expect("request should build"))
        .await
        .expect("router should respond");
    assert_eq!(response.status(), expected_status);

    serde_json::from_str(&response_text(response).await).expect("response should be json")
}

async fn get_text(router: Router, uri: &str) -> String {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(response.status(), StatusCode::OK);

    response_text(response).await
}

async fn response_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");

    String::from_utf8(bytes.to_vec()).expect("body should be utf8")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("relay-knowledge-web-{label}-{now}"))
}
