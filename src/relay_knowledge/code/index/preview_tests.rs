use std::{cell::Cell, fs};

use super::{preview_repository_scope, preview_repository_scope_cancellable};
use crate::{
    code::{
        build_index_snapshot,
        test_fixtures::{TempGitRepo, TempSourceDir},
    },
    domain::{CodeIndexMode, CodeRepositoryRegistration, CodeRepositorySelector},
};

#[test]
fn scope_preview_matches_parser_diagnostics_for_the_same_git_snapshot() {
    let repo = TempGitRepo::create("preview-parser-parity");
    repo.write("src/good.c", "int good(void) { return 1; }\n");
    repo.write(
        "src/external.c",
        "#include <unavailable_header.h>\nint external(void) { return 1; }\n",
    );
    repo.write("src/broken.c", "int broken = ;\n");
    repo.write("src/broken.cpp", "int broken = ;\n");
    repo.write("src/unknown.custom", "unknown\n");
    repo.write("src/large.c", &" ".repeat(512 * 1024 + 1));
    repo.write("src/large.custom", &" ".repeat(512 * 1024 + 1));
    fs::write(repo.path.join("src/binary.c"), b"int x;\0").unwrap();
    fs::write(repo.path.join("src/encoding.c"), b"// \xff\nint x;\n").unwrap();
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "mixed parser outcomes"]);
    // Preview must read committed content, not this now-valid worktree edit.
    repo.write("src/broken.c", "int fixed;\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .unwrap();
    let selector = repo.selector();
    let preview = preview_repository_scope(&registration, &selector).unwrap();
    let indexed =
        build_index_snapshot(&registration, &selector, CodeIndexMode::Full, Vec::new()).unwrap();
    assert_eq!(preview.selected_file_count, 9);
    assert_eq!(preview.expected_degraded_file_count, 7);
    assert_eq!(
        preview.expected_degraded_file_count,
        indexed.diagnostics.len()
    );
    assert_eq!(preview.resolved_commit_sha, indexed.resolved_commit_sha);
    assert_eq!(preview.tree_hash, indexed.tree_hash);
    assert!(
        !indexed
            .diagnostics
            .iter()
            .any(|file| file.path == "src/external.c")
    );
}

#[test]
fn scope_preview_checks_only_selected_files_in_filesystem_snapshots() {
    let source = TempSourceDir::create("preview-filesystem-parity");
    source.write("src/broken.c", "int broken = ;\n");
    source.write("other/broken.c", "int broken = ;\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        source.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .unwrap();
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new()).unwrap();
    let preview = preview_repository_scope(&registration, &selector).unwrap();
    assert_eq!(preview.selected_file_count, 1);
    assert_eq!(preview.expected_degraded_file_count, 1);
    let indexed =
        build_index_snapshot(&registration, &selector, CodeIndexMode::Full, Vec::new()).unwrap();
    assert_eq!(
        preview.expected_degraded_file_count,
        indexed.diagnostics.len()
    );
    assert_eq!(preview.resolved_commit_sha, indexed.resolved_commit_sha);
}

#[test]
fn scope_preview_discards_incomplete_counts_when_cancelled_between_batches() {
    let repo = TempGitRepo::create("preview-cancel");
    repo.write("src/lib.rs", "pub fn valid() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let checks = Cell::new(0);
    let result =
        preview_repository_scope_cancellable(&repo.registration(), &repo.selector(), || {
            checks.set(checks.get() + 1);
            checks.get() > 3
        });
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("cancelled or timed out")
    );
}

#[test]
fn scope_preview_counts_diagnostics_across_multiple_bounded_batches() {
    let repo = TempGitRepo::create("preview-multiple-batches");
    let count = crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1;
    for index in 0..count {
        repo.write(&format!("src/file_{index:04}.c"), "int broken = ;\n");
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "multiple batches"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .unwrap();
    let preview = preview_repository_scope(&registration, &repo.selector()).unwrap();
    assert_eq!(preview.selected_file_count, count);
    assert_eq!(preview.expected_degraded_file_count, count);
}

#[test]
fn scope_preview_pins_git_ref_before_parser_planning() {
    let repo = TempGitRepo::create("preview-moving-head");
    repo.write("src/lib.rs", "pub fn broken() { let x = ; }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "broken"]);
    let checks = Cell::new(0);
    let preview =
        preview_repository_scope_cancellable(&repo.registration(), &repo.selector(), || {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                repo.write("src/lib.rs", "pub fn fixed() {}\n");
                repo.git(["add", "."]);
                repo.git(["commit", "-m", "fixed"]);
            }
            false
        })
        .unwrap();
    assert_eq!(preview.expected_degraded_file_count, 1);
    let current = preview_repository_scope(&repo.registration(), &repo.selector()).unwrap();
    assert_eq!(current.expected_degraded_file_count, 0);
    assert_ne!(preview.resolved_commit_sha, current.resolved_commit_sha);
}

#[test]
fn scope_preview_rejects_filesystem_changes_between_listing_and_parsing() {
    let source = TempSourceDir::create("preview-changing-filesystem");
    source.write("src/lib.rs", "pub fn broken() { let x = ; }\n");
    let checks = Cell::new(0);
    let result =
        preview_repository_scope_cancellable(&source.registration(), &source.selector(), || {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                source.write("src/lib.rs", "pub fn fixed() {}\n");
            }
            false
        });
    assert!(
        result.is_err(),
        "preview must not mix a historical identity with changed bytes"
    );
}
