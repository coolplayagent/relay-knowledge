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
    assert_eq!(preview.expected_degraded_files.len(), 7);
    assert_eq!(
        preview.expected_degraded_files.len(),
        indexed.diagnostics.len()
    );
    assert_eq!(preview.resolved_commit_sha, indexed.resolved_commit_sha);
    assert_eq!(preview.tree_hash, indexed.tree_hash);
    assert!(!preview.expected_degraded_files_truncated);
    for file in &preview.expected_degraded_files {
        let diagnostic = indexed
            .diagnostics
            .iter()
            .find(|item| item.path == file.path)
            .unwrap();
        assert_eq!(file.reason, diagnostic.message);
    }
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
    assert_eq!(preview.expected_degraded_files.len(), 1);
    let indexed =
        build_index_snapshot(&registration, &selector, CodeIndexMode::Full, Vec::new()).unwrap();
    assert_eq!(
        preview.expected_degraded_files.len(),
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
fn code_index_persistence_performance_suite_scope_preview_stops_after_overflow_batch() {
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
    let checks = Cell::new(0);
    let preview = preview_repository_scope_cancellable(&registration, &repo.selector(), || {
        checks.set(checks.get() + 1);
        // Admission, layout completion, then before/after the first parser batch.
        // Any attempt to continue past the overflowing batch must fail this case.
        checks.get() > 4
    })
    .unwrap();
    assert_eq!(preview.selected_file_count, count);
    assert_eq!(preview.expected_degraded_files.len(), 50);
    assert!(preview.expected_degraded_files_truncated);
    assert_eq!(checks.get(), 4);
    for (index, file) in preview.expected_degraded_files.iter().enumerate() {
        assert_eq!(file.path, format!("src/file_{index:04}.c"));
        assert!(!file.reason.is_empty());
    }
    assert!(!preview.excluded_paths_truncated);
}

#[test]
fn scope_preview_degraded_list_distinguishes_exact_limit_from_overflow() {
    for count in [0, 49, 50, 51] {
        let source = TempSourceDir::create("preview-detail-boundary");
        source.write("src/valid.c", "int valid;\n");
        for index in 0..count {
            source.write(&format!("src/file_{index:04}.c"), "int broken = ;\n");
        }
        let preview = preview_repository_scope(&source.registration(), &source.selector()).unwrap();
        assert_eq!(preview.selected_file_count, count + 1);
        assert_eq!(preview.expected_degraded_files.len(), count.min(50));
        assert_eq!(preview.expected_degraded_files_truncated, count > 50);
        assert!(!preview.excluded_paths_truncated);
        let serialized = serde_json::to_value(&preview).unwrap();
        assert!(serialized.get("expected_degraded_file_count").is_none());
        assert!(serialized["expected_degraded_files"].is_array());
        assert_eq!(
            serde_json::from_value::<crate::domain::CodeRepositoryScopePreview>(serialized)
                .unwrap(),
            preview
        );
    }
}

#[test]
fn scope_preview_keeps_checking_after_exactly_fifty_diagnostics() {
    for extra_diagnostic in [false, true] {
        let source = TempSourceDir::create("preview-late-overflow");
        let batch_size = crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH;
        for index in 0..=batch_size {
            let broken = index < 50 || (extra_diagnostic && index == batch_size);
            source.write(
                &format!("src/file_{index:04}.c"),
                if broken {
                    "int broken = ;\n"
                } else {
                    "int valid;\n"
                },
            );
        }
        let preview = preview_repository_scope(&source.registration(), &source.selector()).unwrap();
        assert_eq!(preview.selected_file_count, batch_size + 1);
        assert_eq!(preview.expected_degraded_files.len(), 50);
        assert_eq!(preview.expected_degraded_files_truncated, extra_diagnostic);
        for (index, file) in preview.expected_degraded_files.iter().enumerate() {
            assert_eq!(file.path, format!("src/file_{index:04}.c"));
        }
    }
}

#[test]
fn scope_preview_rejects_source_read_failures_before_truncating_diagnostics() {
    let source = TempSourceDir::create("preview-unreadable-overflow");
    for index in 0..52 {
        source.write(&format!("src/file_{index:04}.c"), "int broken = ;\n");
    }
    let checks = Cell::new(0);
    let result =
        preview_repository_scope_cancellable(&source.registration(), &source.selector(), || {
            checks.set(checks.get() + 1);
            if checks.get() == 3 {
                // Planning is complete, but the first parser batch has not read its files.
                fs::remove_file(source.path.join("src/file_0051.c")).unwrap();
            }
            false
        });
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("source changed or became unreadable")
    );
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
    assert_eq!(preview.expected_degraded_files.len(), 1);
    let current = preview_repository_scope(&repo.registration(), &repo.selector()).unwrap();
    assert_eq!(current.expected_degraded_files.len(), 0);
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
