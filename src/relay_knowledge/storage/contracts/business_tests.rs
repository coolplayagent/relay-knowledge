use super::*;
use crate::domain::{BusinessKnowledgeQueryKind, CodeRepositorySelector, FreshnessPolicy};

struct UnsupportedBusinessStore;

impl BusinessKnowledgeStore for UnsupportedBusinessStore {}

#[tokio::test]
async fn default_business_store_contract_fails_writes_and_reads_closed() {
    let store = UnsupportedBusinessStore;
    let input = BusinessKnowledgeProjectionInput {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        sources: Vec::new(),
    };
    assert!(
        store
            .replace_business_knowledge_projection(input.clone())
            .await
            .is_err()
    );
    assert!(
        store
            .replace_business_knowledge_projection_with_fence(
                input,
                CodeIndexPublicationFence {
                    repository_id: "repo".to_owned(),
                    task_id: "task".to_owned(),
                    lease_owner: "worker".to_owned(),
                    attempt_count: 1,
                    generation: 1,
                },
            )
            .await
            .is_err()
    );
    let request = BusinessKnowledgeQueryRequest::new(
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new()).unwrap(),
        None,
        None,
        BusinessKnowledgeQueryKind::All,
        FreshnessPolicy::AllowStale,
        10,
    )
    .unwrap();
    assert!(
        store
            .business_knowledge_projection_for_scope("git_snapshot:scope".to_owned(), request)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .business_knowledge_status("git_snapshot:scope".to_owned())
            .await
            .unwrap(),
        None
    );
}
