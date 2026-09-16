use super::*;
use crate::{
    domain::{CodeQueryKind, CodebaseViewKind, CodebaseViewRequest, FrameworkGraphRequest},
    storage::FrameworkGraphStore as _,
};

#[tokio::test]
async fn scope_readers_preserve_language_filters_in_shard_and_legacy_control() {
    for published_shard in [false, true] {
        let store = partitioned_store("scope-reader-routing");
        store.upsert_code_repository(registration()).await.unwrap();
        let scope = "scope-routing";
        // Deliberately put different text in control and shard to detect a
        // mistaken fallback to the wrong database after publication.
        let legacy = routing_snapshot(scope, "legacy contract!");
        store
            .control
            .apply_code_index_snapshot(legacy)
            .await
            .unwrap();
        if published_shard {
            indexing::lifecycle::seed_snapshot_for_test(
                &store,
                routing_snapshot(scope, "indexed contract"),
            )
            .await
            .unwrap();
        }
        let expected = if published_shard {
            "indexed contract"
        } else {
            "legacy contract!"
        };
        let documents = store
            .repository_documents_for_scope(scope.into(), vec![], 5, 4096)
            .await
            .unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].content.trim(), expected);
        assert!(
            store
                .repository_documents_for_scope(scope.into(), vec!["excluded".into()], 5, 4096)
                .await
                .unwrap()
                .is_empty()
        );
        for language in ["vue", "python"] {
            let mut selected = selector();
            selected.language_filters = vec![language.into()];
            let query = CodeRetrievalRequest::new(
                "contract",
                selected.clone(),
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap();
            let hits = store.search_code_scope(scope.into(), query).await.unwrap();
            if language == "vue" {
                assert!(!hits.is_empty());
                assert!(
                    hits.iter()
                        .all(|hit| hit.language_id == "vue" && hit.excerpt.contains(expected))
                );
            } else {
                assert!(hits.is_empty());
            }
            let view = CodebaseViewRequest::new(
                selected.clone(),
                CodebaseViewKind::ArchitectureLayers,
                FreshnessPolicy::WaitUntilFresh,
                5,
                vec![],
            )
            .unwrap();
            let snapshot = store
                .codebase_view_snapshot(scope.into(), view, 20)
                .await
                .unwrap();
            assert_eq!(snapshot.files.len(), usize::from(language == "vue"));
            let flags = store
                .search_code_feature_flags_scope(
                    scope.into(),
                    CodeFeatureFlagRequest::new(
                        None,
                        selected.clone(),
                        5,
                        FreshnessPolicy::WaitUntilFresh,
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(flags.len(), usize::from(language == "vue"));
            assert!(flags.iter().all(|flag| flag.name == expected));
            let framework = FrameworkGraphRequest::new(
                None,
                selected.clone(),
                vec![],
                vec![],
                5,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap();
            let graph = store
                .search_framework_graph_scope(scope.into(), framework)
                .await
                .unwrap();
            assert_eq!(graph.nodes.len(), usize::from(language == "vue"));
            assert!(graph.nodes.iter().all(|node| node.name == expected));
            let impact =
                crate::domain::CodeImpactRequest::new(selected, "base", "commit", 5).unwrap();
            let changes = CodeImpactChanges {
                paths: vec!["src/Widget.vue".into()],
                deleted_symbol_names: vec![],
            };
            let impact = store
                .analyze_code_impact_scope(scope.into(), impact, changes)
                .await
                .unwrap();
            assert_eq!(impact.is_empty(), language != "vue");
            assert!(
                impact
                    .iter()
                    .all(|hit| hit.language_id == "vue" && hit.excerpt.contains(expected))
            );
        }
    }
}

fn routing_snapshot(scope: &str, evidence: &str) -> CodeIndexSnapshot {
    use crate::domain::{
        CodeFeatureFlagRecord, CodeFrameworkNodeRecord, FrameworkKind, FrameworkNodeKind,
    };
    let mut data = snapshot(scope);
    let file = &mut data.files[0];
    file.path = "src/Widget.vue".into();
    file.language_id = "vue".into();
    data.chunks[0].path.clone_from(&file.path);
    data.chunks[0].language_id.clone_from(&file.language_id);
    data.chunks[0].content = evidence.into();
    let range = RepositoryCodeRange { start: 0, end: 16 };
    let lines = RepositoryCodeRange { start: 1, end: 1 };
    data.feature_flags.push(CodeFeatureFlagRecord {
        metadata: Default::default(),
        repository_id: "repo".into(),
        source_scope: scope.into(),
        feature_flag_id: "switch".into(),
        usage_id: "read-switch".into(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        language_id: "vue".into(),
        name: evidence.into(),
        source_kind: "env_var".into(),
        source_key: evidence.into(),
        edge_kind: "reads_config".into(),
        confidence_basis_points: 10_000,
        confidence_tier: "exact".into(),
        byte_range: range.clone(),
        line_range: lines.clone(),
        excerpt: evidence.into(),
    });
    data.framework_nodes.push(CodeFrameworkNodeRecord {
        repository_id: "repo".into(),
        source_scope: scope.into(),
        node_id: "component".into(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        framework: FrameworkKind::Vue,
        kind: FrameworkNodeKind::Component,
        name: evidence.into(),
        detail: None,
        symbol_snapshot_id: None,
        byte_range: range,
        line_range: lines,
    });
    let mut document = file.clone();
    document.file_id = "doc".into();
    document.path = "README.md".into();
    document.language_id = "markdown".into();
    let mut chunk = data.chunks[0].clone();
    chunk.file_id = document.file_id.clone();
    chunk.chunk_id = "doc-chunk".into();
    chunk.path.clone_from(&document.path);
    chunk.language_id.clone_from(&document.language_id);
    data.files.push(document);
    data.chunks.push(chunk);
    data
}

#[tokio::test]
async fn report_route_rejects_mismatched_language_and_snapshot_identity() {
    let store = partitioned_store("report-routing-identity");
    let scope = "scope-report-routing";
    store.upsert_code_repository(registration()).await.unwrap();
    indexing::lifecycle::seed_snapshot_for_test(&store, snapshot(scope))
        .await
        .unwrap();
    let report = store
        .code_repository_report("fixture".into())
        .await
        .unwrap();
    assert!(
        routing::report_matches_active_control(
            &store.control,
            &store.catalog,
            "fixture".into(),
            &report
        )
        .await
        .unwrap()
    );
    for field in ["language", "commit", "tree", "count"] {
        let mut changed = report.clone();
        match field {
            "language" => changed.language_filters = vec!["python".into()],
            "commit" => changed.resolved_commit_sha = Some("other".into()),
            "tree" => changed.tree_hash = Some("other".into()),
            _ => changed.indexed_file_count += 1,
        }
        assert!(
            !routing::report_matches_active_control(
                &store.control,
                &store.catalog,
                "fixture".into(),
                &changed
            )
            .await
            .unwrap(),
            "{field}"
        );
    }
}
