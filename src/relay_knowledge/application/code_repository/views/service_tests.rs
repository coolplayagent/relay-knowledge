use super::*;
use crate::application::code_repository::repository::test_support::{
    context, indexed_language_fixture, seed_business_domain,
};
use crate::storage::RepositoryCatalogStore as _;

#[tokio::test]
async fn derived_views_keep_requested_language_and_snapshot_provenance() {
    let (repo, service, store) = indexed_language_fixture(&[]).await;
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    let status = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    seed_business_domain(&store, &status, "authored-java-domain").await;
    for kind in [
        CodebaseViewKind::ArchitectureLayers,
        CodebaseViewKind::BusinessDomains,
        CodebaseViewKind::DependencyTour,
        CodebaseViewKind::ProcessFlow,
        CodebaseViewKind::AffectedScope,
    ] {
        let request = CodebaseViewRequest::new(
            CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["java".into()]).unwrap(),
            kind,
            FreshnessPolicy::WaitUntilFresh,
            20,
            vec!["src/Owner.java".into()],
        )
        .unwrap();
        let response = service
            .codebase_view(request, context("language-view"))
            .await
            .unwrap();
        assert_eq!(response.scope.resolved_commit_sha, commit);
        assert!(!response.scope.stale);
        assert!(
            response.evidence.iter().all(|e| !e.path.ends_with(".py")),
            "{kind:?}: {:?}",
            response.evidence
        );
        assert!(
            response
                .nodes
                .iter()
                .all(|n| n.path.as_deref().is_none_or(|p| !p.ends_with(".py")))
        );
        if kind == CodebaseViewKind::ArchitectureLayers {
            assert!(!response.nodes.is_empty());
        }
        if kind == CodebaseViewKind::DependencyTour {
            assert!(
                response
                    .evidence
                    .iter()
                    .any(|e| e.path == "src/Owner.java" && e.evidence_kind == "file")
            );
            assert!(response.nodes.iter().any(|node| node.node_kind == "module"));
        }
        if kind == CodebaseViewKind::AffectedScope {
            assert!(
                response
                    .evidence
                    .iter()
                    .any(|e| e.path == "src/Owner.java" && e.evidence_kind == "call")
            );
            assert!(
                response
                    .nodes
                    .iter()
                    .any(|node| node.node_kind == "affected_module"
                        && node.path.as_deref() == Some("src/Owner.java"))
            );
        }
        if kind == CodebaseViewKind::ProcessFlow {
            assert!(
                response
                    .nodes
                    .iter()
                    .any(|node| node.label.contains("/java-owner")),
                "{:?}",
                response.nodes
            );
            assert!(
                response
                    .nodes
                    .iter()
                    .all(|node| !node.label.contains("/python-owner"))
            );
        }
        if kind == CodebaseViewKind::BusinessDomains {
            assert!(
                response
                    .nodes
                    .iter()
                    .any(|node| node.label == "authored-java-domain"),
                "{:?}",
                response.nodes
            );
        }
    }
}

#[tokio::test]
async fn views_reject_unindexed_language_even_when_stale_reads_are_allowed() {
    let (_, service, _) = indexed_language_fixture(&["java"]).await;
    for policy in [FreshnessPolicy::WaitUntilFresh, FreshnessPolicy::AllowStale] {
        let request = CodebaseViewRequest::new(
            CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["python".into()]).unwrap(),
            CodebaseViewKind::ArchitectureLayers,
            policy,
            10,
            vec![],
        )
        .unwrap();
        assert!(
            service
                .codebase_view(request, context("missing-language-view"))
                .await
                .is_err()
        );
    }
}
