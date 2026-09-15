use super::*;
#[tokio::test]
async fn diagnostics_http_matches_service_and_rejects_bad_limits() {
    let repo = FixtureRepo::create("web-diagnostics");
    for i in 0..3 {
        repo.write(&format!("bad{i}.py"), "def broken():\n    return (\n");
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "diagnostics"]);
    let fixture_home = repo.path.to_str().unwrap();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [
            ("HOME", fixture_home),
            ("TEMP", fixture_home),
            ("TMP", fixture_home),
            ("TMPDIR", fixture_home),
            ("RELAY_KNOWLEDGE_HOME", fixture_home),
        ],
    )
    .unwrap();
    let runtime = crate::application::RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    let service = RelayKnowledgeService::with_store(
        runtime,
        std::sync::Arc::new(crate::storage::SqliteGraphStore::open_in_memory().unwrap()),
    );
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.to_string_lossy().into_owned(),
                alias: "fixture".into(),
                path_filters: vec![],
                language_filters: vec![],
            },
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .unwrap();
    let selector = CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector.clone(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .unwrap();
    let direct = service
        .code_repository_diagnostics(
            crate::domain::CodeDiagnosticsRequest {
                repository: selector,
                limit: 2,
                cursor: None,
            },
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .unwrap();
    let app = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
    let page = request_json(
        app.clone(),
        "GET",
        "/api/v1/code/repositories/fixture/diagnostics?limit=2",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["diagnostics"], json!(direct.diagnostics));
    assert_eq!(page["next_cursor"], json!(direct.next_cursor));
    assert_eq!(page["degraded_file_count"], 3);
    request_json(
        app,
        "GET",
        "/api/v1/code/repositories/fixture/diagnostics?limit=201",
        None,
        StatusCode::BAD_REQUEST,
    )
    .await;
}
