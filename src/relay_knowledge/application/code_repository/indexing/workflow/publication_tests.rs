use super::effective_publication_lease;
use crate::{
    api::ErrorKind,
    application::code_repository::indexing::task::CodeIndexTaskLeaseContext,
    domain::{CodeIndexPublicationFence, CodeIndexResourceBudget},
};

#[test]
fn effective_lease_rebinds_pending_worktree_identity_without_changing_fence() {
    let pending = pending_lease();
    let actual = effective_publication_lease(
        &pending,
        "repo",
        "git_snapshot:actual".to_owned(),
        "worktree:base:0123456789abcdef".to_owned(),
        "worktree:0123456789abcdef".to_owned(),
    )
    .expect("verified worktree target should become the effective lease");

    assert_eq!(actual.source_scope, "git_snapshot:actual");
    assert_eq!(actual.resolved_commit_sha, "worktree:base:0123456789abcdef");
    assert_eq!(actual.tree_hash, "worktree:0123456789abcdef");
    assert_eq!(actual.publication_fence, pending.publication_fence);
    assert_eq!(actual.resource_budget, pending.resource_budget);
}

#[test]
fn effective_lease_rejects_cross_repository_or_empty_targets() {
    let pending = pending_lease();
    for (repository_id, source_scope, resolved_commit_sha, tree_hash) in [
        ("other", "scope", "commit", "tree"),
        ("repo", "", "commit", "tree"),
        ("repo", "scope", "", "tree"),
        ("repo", "scope", "commit", ""),
    ] {
        let error = effective_publication_lease(
            &pending,
            repository_id,
            source_scope.to_owned(),
            resolved_commit_sha.to_owned(),
            tree_hash.to_owned(),
        )
        .expect_err("invalid target must fail closed");
        assert_eq!(error.error_kind, ErrorKind::Internal);
        assert!(error.message.contains("invalid publication target"));
    }
}

fn pending_lease() -> CodeIndexTaskLeaseContext {
    CodeIndexTaskLeaseContext {
        task_id: "task".to_owned(),
        lease_owner: "worker".to_owned(),
        attempt_count: 1,
        lease_duration_ms: 60_000,
        publication_fence: CodeIndexPublicationFence {
            repository_id: "repo".to_owned(),
            task_id: "task".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            generation: 1,
        },
        source_scope: "git_snapshot:pending".to_owned(),
        resolved_commit_sha: "worktree:pending:base".to_owned(),
        tree_hash: "worktree:pending:base".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        resource_budget: CodeIndexResourceBudget::default(),
    }
}
