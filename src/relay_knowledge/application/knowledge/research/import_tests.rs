use super::super::{
    ResearchService,
    test_support::{TEST_LOCK, fixture, save},
};
use super::*;
use crate::api::InterfaceKind;

#[tokio::test]
async fn round_trips_27_nodes_and_38_relations_without_approving_author_claims() {
    let _guard = TEST_LOCK.lock().await;
    let (root, bundle, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    let response = service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(response.state, "imported");
    assert_eq!(response.fact_status, FactStatus::Proposed);
    let revision = response.audit.bundle_sha256;
    let export = service
        .export_evidence_bundle("study".into(), "research".into(), revision.clone())
        .await
        .unwrap();
    assert_eq!(export.bundle, bundle);
    assert_eq!(export.fact_status, FactStatus::Proposed);
    let again = service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            context,
        )
        .await
        .unwrap();
    assert_eq!(again.state, "already_imported");
    assert_eq!(again.graph_version, response.graph_version);
    assert!(
        service
            .export_evidence_bundle("study".into(), "foreign".into(), revision.clone())
            .await
            .is_err()
    );
    assert!(
        service
            .export_evidence_bundle("study".into(), "research".into(), "bad".into())
            .await
            .is_err()
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn label_clarification_keeps_stable_ids_and_records_proposed_supersession() {
    let _guard = TEST_LOCK.lock().await;
    let (root, bundle, service) = fixture().await;
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    let imported = service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            context.clone(),
        )
        .await
        .unwrap();
    let plan = ResearchService::new(root.clone())
        .revise_bundle(
            "bundle.json".into(),
            "research".into(),
            "concept-0".into(),
            "Clarified category".into(),
        )
        .await
        .unwrap();
    assert!(
        !plan["affected_relation_indices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let revised: AuthoredEvidenceBundle = serde_json::from_value(plan["revision"].clone()).unwrap();
    assert_eq!(revised.graph.nodes[0].id, bundle.graph.nodes[0].id);
    assert_eq!(revised.graph.edges, bundle.graph.edges);
    assert_eq!(revised.aliases["Concept 0"], "concept-0");
    assert_eq!(
        revised.supersedes.as_ref(),
        Some(&imported.audit.bundle_sha256)
    );
    save(&root, &revised).await;
    let next = service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            context,
        )
        .await
        .unwrap();
    assert_eq!(next.fact_status, FactStatus::Proposed);
    let old = service
        .export_evidence_bundle(
            "study".into(),
            "research".into(),
            imported.audit.bundle_sha256,
        )
        .await
        .unwrap();
    assert_eq!(old.bundle, bundle);
    assert_eq!(old.fact_status, FactStatus::Proposed);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn invalid_evidence_never_creates_graph_facts() {
    let _guard = TEST_LOCK.lock().await;
    let (root, _, service) = fixture().await;
    tokio::fs::remove_file(root.join("a.txt")).await.unwrap();
    let response = service
        .import_evidence_bundle(
            root.clone(),
            "bundle.json".into(),
            "research".into(),
            RequestContext::for_interface(InterfaceKind::Cli),
        )
        .await
        .unwrap();
    assert_eq!(response.state, "invalid");
    assert!(response.graph_version.is_none());
    assert!(response.ingest.is_none());
    tokio::fs::remove_dir_all(root).await.unwrap();
}
