use crate::{
    code::test_fixtures::TempGitRepo,
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::preview_repository_scope;

#[test]
fn scope_preview_reports_file_preset_exclusions_and_tracked_directories() {
    let repo = TempGitRepo::create("scope-preview");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("build/workflow.yaml", "steps:\n  - cargo test\n");
    repo.write(".cloudbuild/cloudbuild.yaml", "steps:\n  - name: test\n");
    repo.write("dist/bundle.js", "function generated() {}\n");
    repo.write("data/events.jsonl", "{\"kind\":\"fixture\"}\n");
    repo.write("docs/notes.rs", "fn ignored() {}\n");
    repo.write("manual.pdf", "%PDF-1.7\n");
    repo.write("uv.lock", "version = 1\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
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

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert_eq!(preview.selected_file_count, 6);
    assert_eq!(preview.generated_or_heavy_file_count, 2);
    assert!(
        preview
            .language_distribution
            .iter()
            .any(|language| language.language_id == "rust")
    );
    assert!(
        preview
            .language_distribution
            .iter()
            .any(|language| language.language_id == "python")
    );
    assert!(
        preview
            .largest_files
            .iter()
            .any(|file| file.path == "build/workflow.yaml")
    );
    assert!(
        preview
            .largest_files
            .iter()
            .any(|file| file.path == ".cloudbuild/cloudbuild.yaml")
    );
    assert!(preview.excluded_paths.iter().any(|path| {
        path.path == "data/events.jsonl" && path.reason == "excluded by file preset"
    }));
    assert!(
        preview
            .excluded_paths
            .iter()
            .any(|path| { path.path == "manual.pdf" && path.reason == "excluded by file preset" })
    );
}

#[test]
fn scope_preview_includes_tracked_paths_ignored_by_worktree_gitignore() {
    let repo = TempGitRepo::create("scope-preview-gitignore");
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

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert!(
        preview
            .largest_files
            .iter()
            .any(|file| file.path == "build/workflow.yaml")
    );
}

#[test]
fn scope_preview_counts_each_degraded_file_once() {
    let repo = TempGitRepo::create("scope-preview-degraded-count");
    repo.write("docs/large.custom", &"x".repeat(512 * 1024 + 1));
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
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

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert_eq!(preview.selected_file_count, 1);
    assert_eq!(preview.unsupported_file_count, 1);
    assert_eq!(preview.generated_or_heavy_file_count, 1);
    assert_eq!(preview.expected_degraded_file_count, 1);
}
