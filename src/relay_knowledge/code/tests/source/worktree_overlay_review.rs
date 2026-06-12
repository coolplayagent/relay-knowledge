use super::test_fixtures::TempGitRepo;
use super::*;
use crate::domain::{CodeIndexMode, CodeIndexResourceBudget};
use std::collections::BTreeMap;
use std::fs;

#[cfg(unix)]
#[test]
fn worktree_overlay_deletes_tracked_file_replaced_by_symlink() {
    let repo = TempGitRepo::create("overlay-file-to-symlink");
    repo.write("src/lib.rs", "fn stale_definition() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let outside = repo.path.with_extension("symlink-target");
    fs::write(&outside, "fn outside() {}\n").expect("outside target should be written");
    fs::remove_file(repo.path.join("src/lib.rs")).expect("tracked file should be removed");
    std::os::unix::fs::symlink(&outside, repo.path.join("src/lib.rs"))
        .expect("replacement symlink should be created");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should index symlink replacement");

    assert!(snapshot.deleted_paths.contains(&"src/lib.rs".to_owned()));
    assert!(snapshot.files.iter().all(|file| file.path != "src/lib.rs"));
    fs::remove_file(outside).expect("outside target should be removed");
}

#[test]
fn worktree_overlay_ignores_skipped_untracked_broad_paths_for_limit() {
    let repo = TempGitRepo::create("overlay-skipped-untracked-limit");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    write_many_rust_files(
        &repo,
        "target/generated",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    repo.write("src/new.rs", "fn indexed_new() -> u32 { 1 }\n");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("skipped broad untracked paths should not exhaust overlay limit");

    assert!(snapshot.files.iter().any(|file| file.path == "src/new.rs"));
    assert!(
        snapshot
            .files
            .iter()
            .all(|file| !file.path.starts_with("target/"))
    );
}

#[test]
fn source_grep_worktree_overlay_reads_hash_verified_dirty_bytes() {
    let repo = TempGitRepo::create("overlay-source-grep-dirty");
    repo.write("src/lib.rs", "fn committed_target() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write("src/lib.rs", "fn dirty_target() -> u32 { 1 }\n");
    let bytes = fs::read(repo.path.join("src/lib.rs")).expect("dirty file should read");
    let mut expected_hashes = BTreeMap::new();
    expected_hashes.insert("src/lib.rs".to_owned(), super::stable_content_hash(&bytes));

    let outcome = source_grep_matches_from_worktree_overlay(
        &repo.registration(),
        expected_hashes.clone(),
        source_grep_request("dirty_target"),
    )
    .expect("dirty worktree fallback should read current bytes");

    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/lib.rs");

    repo.write("src/lib.rs", "fn changed_after_index() -> u32 { 2 }\n");
    let stale_outcome = source_grep_matches_from_worktree_overlay(
        &repo.registration(),
        expected_hashes,
        source_grep_request("changed_after_index"),
    )
    .expect("stale dirty worktree fallback should degrade without matches");

    assert!(stale_outcome.matches.is_empty());
}

fn write_many_rust_files(repo: &TempGitRepo, directory: &str, count: usize) {
    for index in 0..count {
        repo.write(
            &format!("{directory}/file_{index}.rs"),
            &format!("pub fn generated_{index}() -> u32 {{ {index} }}\n"),
        );
    }
}

fn source_grep_request(query: &str) -> SourceGrepRequest {
    SourceGrepRequest {
        query: query.to_owned(),
        paths: vec!["src/lib.rs".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::Definition,
        exclude_generated: false,
    }
}
