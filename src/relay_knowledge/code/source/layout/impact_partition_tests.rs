use crate::{
    code::test_fixtures::TempGitRepo,
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::partition_changed_paths_for_selector;

#[test]
fn empty_changed_paths_do_not_read_the_repository() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/path/that/does/not/exist",
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let groups = partition_changed_paths_for_selector(&registration, &selector, Vec::new())
        .expect("empty paths should not touch the repository");

    assert!(groups.in_scope_changed_paths.is_empty());
    assert!(groups.out_of_scope_changed_paths.is_empty());
}

#[test]
fn impact_path_partition_includes_gitignore_ignored_tracked_paths() {
    let repo = TempGitRepo::create("impact-partition-gitignore");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("build/workflow.yaml", "steps:\n  - cargo test\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write(".gitignore", "build\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let groups = partition_changed_paths_for_selector(
        &registration,
        &selector,
        vec![
            "src/lib.rs".to_owned(),
            "build/workflow.yaml".to_owned(),
            "data/events.jsonl".to_owned(),
        ],
    )
    .expect("paths should partition");

    assert_eq!(
        groups.in_scope_changed_paths,
        ["build/workflow.yaml", "src/lib.rs"]
    );
    assert_eq!(groups.out_of_scope_changed_paths, ["data/events.jsonl"]);
}

#[test]
fn impact_path_partition_uses_effective_scope() {
    let repo = TempGitRepo::create("impact-path-groups");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("dist/bundle.js", "function generated() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let groups = partition_changed_paths_for_selector(
        &repo.registration(),
        &repo.selector(),
        vec!["src/lib.rs".to_owned(), "dist/bundle.js".to_owned()],
    )
    .expect("paths should partition");

    assert_eq!(groups.in_scope_changed_paths, ["src/lib.rs"]);
    assert_eq!(groups.out_of_scope_changed_paths, ["dist/bundle.js"]);
}
