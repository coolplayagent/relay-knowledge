use crate::candidate_git::git_repository_fixture::GitRepositoryFixture;

use super::*;

#[test]
fn git_commands_capture_success_and_map_checked_failures() {
    let repository = GitRepositoryFixture::new();

    let result = git(repository.path(), &["rev-parse", "--show-toplevel"], 30);
    assert!(result.passed());
    assert_eq!(
        result.stdout.trim(),
        repository.path().to_string_lossy().as_ref()
    );

    let error = git_checked(repository.path(), &["rev-parse", "missing-ref"], 30)
        .expect_err("missing ref should fail");
    assert!(error.contains("git"));
}
