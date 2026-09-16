use super::*;
use crate::application::code_repository::repository::test_support::{
    context, indexed_language_fixture,
};
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[tokio::test]
async fn context_expansion_pins_language_evidence_and_respects_code_visibility() {
    let (repo, service, _) = indexed_language_fixture(&[]).await;
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    for include_code in [true, false] {
        let request = CodeGraphContextRequest::new(
            CodeRepositorySelector::new("fixture", "HEAD", vec!["src".into()], vec!["java".into()])
                .unwrap(),
            "Owner language:java",
            10,
            FreshnessPolicy::WaitUntilFresh,
            32_768,
            include_code,
            false,
        )
        .unwrap();
        let response = service
            .codegraph_context(request, context("language-context"))
            .await
            .unwrap();
        assert_eq!(response.repository_scope.resolved_commit_sha, commit);
        assert!(!response.pack.entry_points.is_empty());
        assert!(
            !response.pack.graph_paths.is_empty(),
            "context must retain expansion evidence"
        );
        if include_code {
            assert!(
                response
                    .pack
                    .graph_paths
                    .iter()
                    .any(|hit| hit.excerpt.contains("run calls target")),
                "{:?}",
                response.pack.graph_paths
            );
        }
        let hits = response
            .pack
            .entry_points
            .iter()
            .chain(&response.pack.related_symbols)
            .chain(&response.pack.graph_paths)
            .collect::<Vec<_>>();
        assert!(hits.iter().all(|hit| hit.path == "src/Owner.java"
            && hit.language_id == "java"
            && hit.resolved_commit_sha == commit
            && hit.scope_id == response.repository_scope.scope_id));
        assert!(response.budget.context_bytes <= response.budget.max_context_bytes);
        if include_code {
            assert!(!response.pack.code_excerpts.is_empty());
        } else {
            assert!(response.pack.code_excerpts.is_empty());
            assert!(hits.iter().all(|hit| hit.excerpt.is_empty()));
        }
    }
}

#[tokio::test]
async fn context_does_not_reuse_a_snapshot_that_lacks_the_requested_language() {
    let (_, service, _) = indexed_language_fixture(&["java"]).await;
    let request = CodeGraphContextRequest::new(
        CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["python".into()]).unwrap(),
        "Owner",
        3,
        FreshnessPolicy::WaitUntilFresh,
        1024,
        false,
        false,
    )
    .unwrap();
    let error = service
        .codegraph_context(request, context("missing-language-context"))
        .await
        .unwrap_err();
    assert!(
        error.message.contains("scope") || error.message.contains("index"),
        "{error:?}"
    );
}

#[tokio::test]
async fn context_empty_search_has_no_fabricated_graph_evidence() {
    let (_, service, _) = indexed_language_fixture(&["java"]).await;
    let request = CodeGraphContextRequest::new(
        CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["java".into()]).unwrap(),
        "unfindable_unique_query",
        1,
        FreshnessPolicy::WaitUntilFresh,
        1024,
        true,
        false,
    )
    .unwrap();
    let response = service
        .codegraph_context(request, context("empty-language-context"))
        .await
        .unwrap();
    assert!(response.pack.entry_points.is_empty());
    assert!(response.pack.graph_paths.is_empty());
    assert_eq!(response.budget.returned_count, 0);
    assert!(!response.truncated);
}
