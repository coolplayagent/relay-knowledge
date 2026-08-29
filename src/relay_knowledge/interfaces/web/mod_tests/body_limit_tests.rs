use super::*;

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
