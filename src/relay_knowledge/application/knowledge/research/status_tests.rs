use super::super::test_support::{TEST_LOCK, fixture};
use super::*;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
};

fn request(root: &std::path::Path, delivery: ResearchDelivery) -> ResearchStatusRequest {
    ResearchStatusRequest {
        root: root.into(),
        delivery,
        catalog: None,
        bundle: None,
        scope: None,
        requirements: None,
    }
}

async fn initialize_map(root: &std::path::Path, context: &RequestContext) {
    let map = KnowledgeMapService::new(root.into());
    map.init(context).await.unwrap();
    map.for_type(crate::domain::RepositoryMapType::Codespec)
        .init(context)
        .await
        .unwrap();
    tokio::fs::write(
        root.join("AGENTS.md"),
        "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn maps_alone_never_complete_archive_or_graphrag_and_graph_delivery_can_stay_unimported() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    initialize_map(&root, &context).await;
    for delivery in [ResearchDelivery::Archive, ResearchDelivery::Graphrag] {
        let report = service
            .research_status(request(&root, delivery), context.clone())
            .await
            .unwrap();
        assert!(report.map.valid, "{:?}", report.map.diagnostics);
        assert_eq!(report.repository_index.state, "not_indexed");
        assert_eq!(report.readiness, "needs_action");
        assert_eq!(report.content_verdict, "unknown");
        if delivery == ResearchDelivery::Archive {
            assert!(
                !report
                    .next_steps
                    .iter()
                    .any(|step| step.contains("Register/index"))
            );
        }
    }
    let mut input = request(&root, ResearchDelivery::AuthoredGraph);
    input.bundle = Some("bundle.json".into());
    input.scope = Some("research".into());
    let report = service
        .research_status(input.clone(), context.clone())
        .await
        .unwrap();
    assert_eq!(report.bundle.as_ref().unwrap().import_state, "not_imported");
    assert_eq!(report.readiness, "ready_for_review");
    input.delivery = ResearchDelivery::Graphrag;
    assert_eq!(
        service
            .research_status(input.clone(), context.clone())
            .await
            .unwrap()
            .readiness,
        "needs_action"
    );
    service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            context.clone(),
        )
        .await
        .unwrap();
    let report = service.research_status(input, context).await.unwrap();
    assert_eq!(
        report.bundle.as_ref().unwrap().stored_fact_status,
        Some(crate::domain::FactStatus::Proposed)
    );
    assert_eq!(report.readiness, "ready_for_review");
    assert_eq!(report.repository_index.state, "not_indexed");
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn requirement_hashes_and_archive_integrity_do_not_prove_content_completion() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    initialize_map(&root, &context).await;
    let artifact = serde_json::json!({"path_base":"repository","path":"a.txt","sha256":reader::digest(b"First\nSecond\n")});
    let catalog = serde_json::json!({"schema_version":1,"adapter":"relay-capture-v1","sources":[{"id":"a","url":"https://example.org","raw":artifact}]});
    tokio::fs::write(
        root.join("catalog.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .await
    .unwrap();
    let manifest = serde_json::json!({"schema_version":1,"requirements":[{"id":"body","description":"Full article is captured","evidence":[artifact]}]});
    tokio::fs::write(
        root.join("requirements.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .await
    .unwrap();
    let mut input = request(&root, ResearchDelivery::Archive);
    input.catalog = Some("catalog.json".into());
    input.requirements = Some("requirements.json".into());
    let report = service
        .research_status(input.clone(), context.clone())
        .await
        .unwrap();
    assert_eq!(report.readiness, "ready_for_review");
    let requirement = &report.requirements.as_ref().unwrap().requirements[0];
    assert_eq!(requirement.evidence_integrity, "verified");
    assert_eq!(requirement.content_verdict, "unknown");
    tokio::fs::write(root.join("a.txt"), b"changed")
        .await
        .unwrap();
    let report = service.research_status(input, context).await.unwrap();
    assert_eq!(report.readiness, "needs_action");
    assert_eq!(
        report.requirements.unwrap().requirements[0].evidence_integrity,
        "invalid"
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn matches_only_this_root_and_keeps_fresh_partial_and_stale_states_distinct() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    initialize_map(&root, &context).await;
    tokio::fs::create_dir_all(root.join("src")).await.unwrap();
    tokio::fs::write(root.join("src/lib.rs"), "pub fn ready() {}\n")
        .await
        .unwrap();
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: root.to_string_lossy().into_owned(),
                alias: "research-fixture".into(),
                path_filters: vec!["src".into()],
                language_filters: Vec::new(),
            },
            context.clone(),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(
                    "research-fixture",
                    "HEAD",
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context.clone(),
        )
        .await
        .unwrap();
    let mut report = service
        .research_status(request(&root, ResearchDelivery::Graphrag), context.clone())
        .await
        .unwrap();
    assert_eq!(report.repository_index.state, "fresh");
    assert_eq!(
        report.repository_index.registration.as_ref().unwrap().alias,
        "research-fixture"
    );
    assert_eq!(report.readiness, "ready_for_review");
    // An explicitly selected malformed bundle must not fall back to an unrelated
    // fresh code index when deciding GraphRAG readiness.
    report.bundle_error = Some("invalid selected bundle".into());
    evaluate_readiness(&mut report);
    assert_eq!(report.readiness, "needs_action");
    report.bundle_error = None;
    report
        .repository_index
        .served_scope
        .as_mut()
        .unwrap()
        .content_integrity
        .state = CodeContentIntegrityState::Partial;
    report.next_steps.clear();
    evaluate_readiness(&mut report);
    assert_eq!(report.repository_index.state, "fresh");
    assert_eq!(report.readiness, "needs_action");
    let unrelated = root.join("unrelated");
    tokio::fs::create_dir_all(&unrelated).await.unwrap();
    let unrelated = std::fs::canonicalize(unrelated).unwrap();
    assert_eq!(
        service
            .research_repository_state(unrelated.to_string_lossy().into_owned())
            .await
            .unwrap()
            .state,
        "not_indexed"
    );
    tokio::fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n")
        .await
        .unwrap();
    let report = service
        .research_status(request(&root, ResearchDelivery::Graphrag), context)
        .await
        .unwrap();
    assert_eq!(report.repository_index.state, "stale");
    assert_eq!(report.readiness, "needs_action");
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn malformed_optional_artifacts_are_reported_without_hiding_other_layers() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    let mut input = request(&root, ResearchDelivery::AuthoredGraph);
    input.catalog = Some("absent.json".into());
    input.bundle = Some("bundle.json".into());
    input.requirements = Some("absent.json".into());
    let report = service
        .research_status(input, RequestContext::for_interface(InterfaceKind::Cli))
        .await
        .unwrap();
    assert!(!report.map.valid);
    assert!(report.sources_error.is_some());
    assert!(report.bundle_error.is_some());
    assert!(report.requirements_error.is_some());
    assert_eq!(report.repository_index.state, "not_indexed");
    assert_eq!(report.readiness, "needs_action");
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn standalone_artifact_deliveries_do_not_require_navigation_maps() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    let mut input = request(&root, ResearchDelivery::AuthoredGraph);
    input.bundle = Some("bundle.json".into());
    input.scope = Some("research".into());
    let report = service
        .research_status(input, context.clone())
        .await
        .unwrap();
    assert!(!report.map.valid);
    assert_eq!(report.readiness, "ready_for_review");
    assert!(
        report
            .next_steps
            .iter()
            .any(|step| step.contains("navigation map"))
    );
    let catalog = serde_json::json!({"schema_version":1,"adapter":"relay-capture-v1","sources":[{"id":"raw","url":"https://example.org","raw":{"path_base":"repository","path":"a.txt","sha256":super::super::reader::digest(b"First\nSecond\n")}}]});
    tokio::fs::write(
        root.join("catalog.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .await
    .unwrap();
    let mut input = request(&root, ResearchDelivery::Archive);
    input.catalog = Some("catalog.json".into());
    let report = service.research_status(input, context).await.unwrap();
    assert!(!report.map.valid);
    assert_eq!(report.readiness, "ready_for_review");
    assert_eq!(report.repository_index.state, "not_indexed");
    tokio::fs::remove_dir_all(root).await.unwrap();
}
