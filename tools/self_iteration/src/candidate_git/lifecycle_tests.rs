use std::path::PathBuf;

use crate::candidate_git::git_repository_fixture::GitRepositoryFixture;

use super::*;

#[test]
fn commit_candidate_stages_all_changes_and_returns_new_head() {
    let repository = GitRepositoryFixture::new();
    let base_ref = repository.head();
    repository.write("tracked.txt", "accepted\n");
    repository.write("new.txt", "accepted\n");

    let commit = commit_candidate(
        repository.path(),
        Some("Accept fixture candidate"),
        1.0,
        &base_ref,
    )
    .expect("candidate should commit");

    assert_eq!(commit, repository.short_head());
    assert!(repository.status().is_empty());
    assert_eq!(
        repository.run(&["log", "-1", "--pretty=%s"]).trim(),
        "Accept fixture candidate"
    );
}

#[test]
fn hard_rejection_restores_base_and_removes_untracked_files() {
    let repository = GitRepositoryFixture::new();
    let base_ref = repository.head();
    repository.write("tracked.txt", "rejected\n");
    repository.write("untracked.txt", "rejected\n");
    let patch = PatchSnapshot {
        path: PathBuf::from("unused.patch"),
        diff: String::new(),
        sha256: String::new(),
        base_ref,
    };

    reject_candidate(repository.path(), &patch, true).expect("candidate should be rejected");

    assert!(repository.status().is_empty());
    assert_eq!(repository.read("tracked.txt"), "initial\n");
    assert!(!repository.path().join("untracked.txt").exists());
}
