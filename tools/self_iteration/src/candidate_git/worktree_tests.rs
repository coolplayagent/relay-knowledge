use crate::candidate_git::git_repository_fixture::GitRepositoryFixture;

use super::*;

#[test]
fn clean_worktree_and_head_checks_follow_repository_state() {
    let repository = GitRepositoryFixture::new();

    ensure_clean_worktree(repository.path()).expect("fixture should start clean");
    assert_eq!(
        current_head(repository.path()).expect("head should resolve"),
        repository.head()
    );

    repository.write("tracked.txt", "changed\n");
    let error = ensure_clean_worktree(repository.path()).expect_err("dirty tree should fail");
    assert!(error.contains("--use-current-candidate"));
}
