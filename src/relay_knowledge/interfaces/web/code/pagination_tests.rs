//! Direct Web route pagination must survive request normalization.
use super::*;

#[test]
fn software_cursor_normalization_preserves_and_validates_continuations() {
    let mut request = SoftwareGlobalRequest::new(
        CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap(),
        SoftwareGlobalKind::Dependencies,
        FreshnessPolicy::AllowStale,
        1,
    )
    .unwrap()
    .with_cursor(Some("opaque-token".into()))
    .unwrap();
    assert!(super::super::normalize_software_request(&mut request).is_none());
    assert_eq!(request.cursor.as_deref(), Some("opaque-token"));
    request.kind = SoftwareGlobalKind::Build;
    assert!(super::super::normalize_software_request(&mut request).is_some());
    request.kind = SoftwareGlobalKind::Modules;
    request.cursor = Some("x".repeat(4097));
    assert!(super::super::normalize_software_request(&mut request).is_some());
}

#[tokio::test]
async fn maven_direct_web_endpoint_follows_cursor_without_repeating_first_page() {
    let repo = FixtureRepo::create("web-maven-pages");
    repo.write("pom.xml", "<project><groupId>x</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>child</module></modules></project>");
    repo.write("child/pom.xml", "<project><parent><groupId>x</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>child</artifactId></project>");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "reactor"]);
    let service = test_service("web-maven-pages").await;
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
    let router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
    for kind in [
        SoftwareGlobalKind::Modules,
        SoftwareGlobalKind::Dependencies,
    ] {
        let mut request =
            SoftwareGlobalRequest::new(selector.clone(), kind, FreshnessPolicy::WaitUntilFresh, 1)
                .unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..10 {
            let page = request_json(
                router.clone(),
                "POST",
                "/api/v1/code/repositories/fixture/software",
                Some(json!(request)),
                StatusCode::OK,
            )
            .await;
            for (field, key) in [
                ("build_targets", "target_id"),
                ("relationships", "relationship_id"),
                ("components", "component_id"),
                ("dependency_usages", "usage_id"),
            ] {
                for fact in page[field].as_array().unwrap() {
                    assert!(
                        ids.insert(fact[key].as_str().unwrap().to_owned()),
                        "Web cursor repeated a fact: {page}"
                    );
                }
            }
            request.cursor = page["next_cursor"].as_str().map(str::to_owned);
            if request.cursor.is_none() {
                break;
            }
        }
        assert!(request.cursor.is_none());
        assert_eq!(ids.len(), 4);
    }
}
