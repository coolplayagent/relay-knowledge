use super::*;
use crate::api::CodeRepositoryRegisterRequest;
use crate::application::code_repository::repository::test_support::{
    FixtureRepo, context, service_with_memory_store,
};
use crate::domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector};

#[tokio::test]
async fn framework_service_keeps_language_and_snapshot_evidence_across_policies() {
    let repo = FixtureRepo::create("framework-service-scope");
    repo.write("src/app.ts", "import {Component} from '@angular/core';\n@Component({selector: 'app-root', template: '<span>hello</span>'})\nexport class AppComponent {}\n");
    repo.write(
        "src/Other.vue",
        "<template><div>other</div></template>\n<script setup>const label = 'other';</script>\n",
    );
    repo.git(["add", "src"]);
    repo.git(["commit", "-m", "framework scope evidence"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: vec!["src".into()],
                language_filters: vec![],
            },
            context("register-framework-scope"),
        )
        .await
        .unwrap();
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-framework-scope"),
        )
        .await
        .unwrap();
    for policy in [
        FreshnessPolicy::WaitUntilFresh,
        FreshnessPolicy::AllowStale,
        FreshnessPolicy::GraphOnly,
    ] {
        for (language, expected_path) in [
            ("typescript", "src/app.ts"),
            ("vue", "src/Other.vue"),
            ("java", ""),
        ] {
            let request = FrameworkGraphRequest::new(
                None,
                CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![language.into()])
                    .unwrap(),
                vec![],
                vec![],
                20,
                policy,
            )
            .unwrap();
            let response = service
                .query_code_repository_framework_graph(request, context("query-framework-scope"))
                .await
                .unwrap();
            assert_eq!(response.scope.resolved_commit_sha, commit);
            if policy == FreshnessPolicy::GraphOnly {
                assert!(response.graph.nodes.is_empty());
                assert!(response.graph.edges.is_empty());
                assert!(
                    response
                        .degraded_reason
                        .as_deref()
                        .unwrap()
                        .contains("graph_only")
                );
            } else {
                assert_eq!(response.scope.scope_id, indexed.scope.scope_id);
                assert!(!response.scope.stale);
                if language == "java" {
                    assert!(response.graph.nodes.is_empty());
                    assert!(response.graph.edges.is_empty());
                } else {
                    assert!(!response.graph.nodes.is_empty(), "{language}");
                    assert!(
                        response
                            .graph
                            .nodes
                            .iter()
                            .all(|node| node.path == expected_path)
                    );
                    assert!(
                        response
                            .graph
                            .edges
                            .iter()
                            .all(|edge| edge.path == expected_path)
                    );
                    if language == "typescript" {
                        assert!(
                            response
                                .graph
                                .nodes
                                .iter()
                                .any(|node| node.name == "AppComponent")
                        );
                        assert!(!response.graph.edges.is_empty());
                    } else {
                        assert!(response.graph.nodes.iter().any(|node| node.name == "Other"
                            && node.kind == crate::domain::FrameworkNodeKind::Component));
                        assert!(response.graph.nodes.iter().any(|node| node.kind == crate::domain::FrameworkNodeKind::Template));
                        assert!(response.graph.edges.iter().any(
                            |edge| edge.kind == crate::domain::FrameworkEdgeKind::OwnsTemplate
                        ));
                    }
                }
            }
        }
    }
    let request = FrameworkGraphRequest::new(
        None,
        CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            vec!["src/Other.vue".into()],
            vec!["typescript".into()],
        )
        .unwrap(),
        vec![],
        vec![],
        20,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    let response = service
        .query_code_repository_framework_graph(
            request,
            context("framework-path-language-intersection"),
        )
        .await
        .unwrap();
    assert!(response.graph.nodes.is_empty());
    assert!(response.graph.edges.is_empty());
}

#[tokio::test]
async fn framework_service_rejects_a_language_missing_from_the_published_scope() {
    let (_repo, service, _) =
        crate::application::code_repository::repository::test_support::indexed_language_fixture(&[
            "java",
        ])
        .await;
    for policy in [FreshnessPolicy::WaitUntilFresh, FreshnessPolicy::AllowStale] {
        let request = FrameworkGraphRequest::new(
            None,
            CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["vue".into()]).unwrap(),
            vec![],
            vec![],
            20,
            policy,
        )
        .unwrap();
        assert!(
            service
                .query_code_repository_framework_graph(
                    request,
                    context("missing-framework-language")
                )
                .await
                .is_err()
        );
    }
}
