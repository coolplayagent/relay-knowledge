use super::*;
use crate::application::code_repository::repository::test_support::{
    context, indexed_language_fixture, seed_business_domain,
};
use crate::domain::{
    BusinessKnowledgeQueryKind, CodeIndexMode, CodeIndexRequest, CodeRepositorySelector,
};
use crate::storage::RepositoryCatalogStore as _;

#[tokio::test]
async fn business_projection_uses_the_same_authorized_language_scope() {
    let (repo, service, store) = indexed_language_fixture(&["java"]).await;
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    let java = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    seed_business_domain(&store, &java, "java-domain").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec![],
                    vec!["python".into()],
                )
                .unwrap(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-other-business-scope"),
        )
        .await
        .unwrap();
    let python = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    seed_business_domain(&store, &python, "python-domain").await;
    for policy in [
        FreshnessPolicy::GraphOnly,
        FreshnessPolicy::WaitUntilFresh,
        FreshnessPolicy::AllowStale,
    ] {
        let request = BusinessKnowledgeQueryRequest::new(
            CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["java".into()]).unwrap(),
            None,
            None,
            BusinessKnowledgeQueryKind::All,
            policy,
            10,
        )
        .unwrap();
        let response = service
            .business_knowledge_query(request, context("language-business"))
            .await
            .unwrap();
        assert_eq!(response.scope.resolved_commit_sha, commit);
        if policy == FreshnessPolicy::GraphOnly {
            assert!(response.terms.is_empty());
            assert_eq!(
                response.result.status,
                BusinessKnowledgeResultStatus::Unavailable
            );
            assert_eq!(response.knowledge.state, BusinessKnowledgeState::Unknown);
        } else {
            assert_eq!(
                response.scope.scope_id,
                java.last_indexed_scope_id.clone().unwrap()
            );
            assert_eq!(response.domains.len(), 1);
            assert_eq!(response.domains[0].id, "java-domain");
            assert_eq!(response.terms.len(), 1);
            assert!(!response.scope.stale);
            assert_eq!(response.scope.language_filters, ["java"]);
        }
    }
    for policy in [FreshnessPolicy::WaitUntilFresh, FreshnessPolicy::AllowStale] {
        let request = BusinessKnowledgeQueryRequest::new(
            CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["go".into()]).unwrap(),
            None,
            None,
            BusinessKnowledgeQueryKind::All,
            policy,
            10,
        )
        .unwrap();
        assert!(
            service
                .business_knowledge_query(request, context("missing-language-business"))
                .await
                .is_err()
        );
    }
}
