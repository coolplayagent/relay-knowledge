use crate::candidate_git::git_repository_fixture::GitRepositoryFixture;

use super::*;

#[test]
fn dynamic_git_commands_apply_the_requested_check_policy() {
    let repository = GitRepositoryFixture::new();
    let success = git_dynamic(
        repository.path(),
        &["rev-parse".to_owned(), "HEAD".to_owned()],
        30,
        true,
    )
    .expect("existing head should pass");
    assert_eq!(success.stdout.trim(), repository.head());

    let unchecked = git_dynamic(
        repository.path(),
        &["rev-parse".to_owned(), "missing-ref".to_owned()],
        30,
        false,
    )
    .expect("unchecked result should be returned");
    assert!(!unchecked.passed());

    assert!(
        git_dynamic(
            repository.path(),
            &["rev-parse".to_owned(), "missing-ref".to_owned()],
            30,
            true,
        )
        .is_err()
    );
}
