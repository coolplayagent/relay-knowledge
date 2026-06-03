use super::test_fixtures::TempGitRepo;
use super::*;
use std::fs;

#[test]
fn worktree_overlay_hash_tracks_modified_content() {
    let repo = TempGitRepo::create("overlay-hash");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let registration = repo.registration();
    let selector = repo.selector();

    repo.write("src/lib.rs", "fn value() -> u32 { 1 }\n");
    let first = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("first overlay should index");

    repo.write("src/lib.rs", "fn value() -> u32 { 2 }\n");
    let second = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("second overlay should index");

    assert_ne!(first.tree_hash, second.tree_hash);
}

#[test]
fn clean_worktree_overlay_rebuilds_clean_commit_snapshot() {
    let repo = TempGitRepo::create("overlay-clean");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("clean overlay should index clean commit");

    assert!(snapshot.full_replace);
    assert_eq!(snapshot.resolved_commit_sha, head);
    assert_eq!(snapshot.files.len(), 1);
}

#[test]
fn dirty_worktree_overlay_uses_synthetic_commit_identity() {
    let repo = TempGitRepo::create("overlay-dirty-identity");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    repo.write("src/lib.rs", "fn value() -> u32 { 1 }\n");
    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("dirty overlay should index");

    assert!(snapshot.resolved_commit_sha.starts_with("worktree:"));
    assert!(snapshot.tree_hash.starts_with("worktree:"));
}

#[test]
fn worktree_overlay_hash_ignores_out_of_scope_changes() {
    let repo = TempGitRepo::create("overlay-out-of-scope");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.write("docs/readme.md", "first\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    repo.write("docs/readme.md", "second\n");
    let first = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("first overlay should index");

    repo.write("docs/readme.md", "third\n");
    let second = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("second overlay should index");

    assert_eq!(first.tree_hash, second.tree_hash);
    assert!(!first.resolved_commit_sha.starts_with("worktree:"));
    assert!(first.files.iter().any(|file| file.path == "src/lib.rs"));
}

#[test]
fn worktree_overlay_indexes_untracked_files_under_new_directories() {
    let repo = TempGitRepo::create("overlay-untracked-all");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.git(["config", "status.showUntrackedFiles", "no"]);
    let content = "fn value() -> u32 { 1 }\n";
    repo.write("src/lib.rs", content);
    repo.write("src/generated/temp.rs", "fn generated() {}\n");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        vec![
            CodeFileFingerprint {
                path: "src/lib.rs".to_owned(),
                blob_hash: stable_content_hash(content.as_bytes()),
            },
            CodeFileFingerprint {
                path: "src/old.rs".to_owned(),
                blob_hash: "stale".to_owned(),
            },
        ],
    )
    .expect("overlay should index untracked files despite local status config");

    assert_eq!(snapshot.skipped_unchanged_count, 1);
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/generated/temp.rs")
    );
}

#[test]
fn worktree_overlay_respects_gitignore_for_untracked_directories() {
    let repo = TempGitRepo::create("overlay-gitignore");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.write(".gitignore", "build/\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write("src/local.rs", "fn local_value() -> u32 { 1 }\n");
    repo.write("build/generated.rs", "fn ignored_generated() {}\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");

    let snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should use git status ignore rules");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/local.rs")
    );
    assert!(
        snapshot
            .files
            .iter()
            .all(|file| file.path != "build/generated.rs")
    );
}

#[test]
fn worktree_overlay_skips_untracked_broad_directories_without_path_opt_in() {
    let repo = TempGitRepo::create("overlay-untracked-broad");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write("src/local.rs", "fn local_value() -> u32 { 1 }\n");
    repo.write("vendor/pkg/lib.rs", "pub fn vendored() {}\n");
    repo.write(
        "node_modules/pkg/index.js",
        "export function dependency() {}\n",
    );
    repo.write("target/generated.rs", "fn generated() {}\n");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should avoid broad untracked trees");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/local.rs")
    );
    for path in [
        "vendor/pkg/lib.rs",
        "node_modules/pkg/index.js",
        "target/generated.rs",
    ] {
        assert!(
            snapshot.files.iter().all(|file| file.path != path),
            "{path}"
        );
    }
}

#[test]
fn worktree_overlay_allows_explicit_untracked_broad_path_opt_in() {
    let repo = TempGitRepo::create("overlay-untracked-broad-opt-in");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write("vendor/pkg/lib.rs", "pub fn vendored() {}\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec!["vendor/pkg".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");

    let snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should honor explicit broad path opt-in");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "vendor/pkg/lib.rs")
    );
}

#[cfg(unix)]
#[test]
fn worktree_overlay_skips_symlinked_directories() {
    let repo = TempGitRepo::create("overlay-symlink-dir");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let outside = repo.path.with_extension("outside-target");
    fs::create_dir_all(&outside).expect("outside target should exist");
    fs::write(outside.join("secret.rs"), "fn secret() {}\n")
        .expect("outside source should be written");
    std::os::unix::fs::symlink(&outside, repo.path.join("src/link"))
        .expect("directory symlink should be created");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should not traverse symlink directories");

    assert!(
        snapshot
            .files
            .iter()
            .all(|file| !file.path.starts_with("src/link/"))
    );
    fs::remove_dir_all(outside).expect("outside target should be removed");
}

#[cfg(unix)]
#[test]
fn worktree_overlay_ignores_out_of_language_dangling_symlinks() {
    let repo = TempGitRepo::create("overlay-dangling-symlink");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    std::os::unix::fs::symlink(repo.path.join("missing-target"), repo.path.join("src/bad"))
        .expect("dangling symlink should be created");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("dangling symlinks outside language filters should not fail overlay indexing");

    assert!(snapshot.files.iter().all(|file| file.path != "src/bad"));
}

#[test]
fn worktree_overlay_uses_committed_submodule_snapshot_without_dirty_overlay() {
    let source = TempGitRepo::create("overlay-submodule-source");
    source.write("lib.rs", "fn submodule_value() -> u32 { 0 }\n");
    source.git(["add", "."]);
    source.git(["commit", "-m", "initial"]);
    let repo = TempGitRepo::create("overlay-submodule-parent");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let source_path = source.path.display().to_string();
    repo.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &source_path,
        "src/submodule",
    ]);
    repo.git(["commit", "-am", "add submodule"]);
    fs::write(
        repo.path.join("src/submodule/lib.rs"),
        "fn submodule_value() -> u32 { 1 }\n",
    )
    .expect("submodule worktree should be modified");

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should not recurse into modified submodules");

    assert!(snapshot.full_replace);
    assert!(!snapshot.resolved_commit_sha.starts_with("worktree:"));
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/submodule/lib.rs")
    );
}

#[test]
fn worktree_overlay_indexes_staged_submodule_gitlink_update() {
    let source = TempGitRepo::create("overlay-staged-submodule-source");
    source.write("lib.rs", "fn submodule_value() -> u32 { 0 }\n");
    source.git(["add", "."]);
    source.git(["commit", "-m", "initial"]);
    let repo = TempGitRepo::create("overlay-staged-submodule-parent");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let source_path = source.path.display().to_string();
    repo.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &source_path,
        "src/submodule",
    ]);
    repo.git(["commit", "-am", "add submodule"]);
    let submodule = TempGitRepo {
        path: repo.path.join("src/submodule"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    submodule.write("lib.rs", "fn staged_submodule_value() -> u32 { 1 }\n");
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "submodule update"]);
    repo.git(["add", "src/submodule"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should index staged submodule gitlink updates");

    assert!(!snapshot.full_replace);
    assert!(snapshot.resolved_commit_sha.starts_with("worktree:"));
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/submodule/lib.rs")
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "staged_submodule_value")
    );
}

#[test]
fn worktree_overlay_indexes_staged_submodule_gitlink_update_after_deinit() {
    let source = TempGitRepo::create("overlay-staged-deinit-submodule-source");
    source.write("lib.rs", "fn submodule_value() -> u32 { 0 }\n");
    source.git(["add", "."]);
    source.git(["commit", "-m", "initial"]);
    let repo = TempGitRepo::create("overlay-staged-deinit-submodule-parent");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let source_path = source.path.display().to_string();
    repo.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &source_path,
        "src/submodule",
    ]);
    repo.git(["commit", "-am", "add submodule"]);
    let submodule = TempGitRepo {
        path: repo.path.join("src/submodule"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    submodule.write(
        "lib.rs",
        "fn staged_deinit_submodule_value() -> u32 { 1 }\n",
    );
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "submodule update"]);
    repo.git(["add", "src/submodule"]);
    repo.git(["submodule", "deinit", "-f", "src/submodule"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should index staged submodule gitlinks after deinit");

    assert!(!snapshot.full_replace);
    assert!(snapshot.resolved_commit_sha.starts_with("worktree:"));
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/submodule/lib.rs")
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "staged_deinit_submodule_value")
    );
}

#[test]
fn worktree_overlay_expands_staged_submodule_deletions() {
    let source = TempGitRepo::create("overlay-delete-submodule-source");
    source.write("lib.rs", "fn deleted_submodule_value() -> u32 { 0 }\n");
    source.git(["add", "."]);
    source.git(["commit", "-m", "initial"]);
    let repo = TempGitRepo::create("overlay-delete-submodule-parent");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let source_path = source.path.display().to_string();
    repo.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &source_path,
        "src/submodule",
    ]);
    repo.git(["commit", "-am", "add submodule"]);
    repo.git(["rm", "-f", "src/submodule"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should expand staged submodule deletions");

    assert!(!snapshot.full_replace);
    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/submodule/lib.rs".to_owned())
    );
    assert!(!snapshot.deleted_paths.contains(&"src/submodule".to_owned()));
}

#[test]
fn worktree_overlay_skips_untracked_nested_git_repositories() {
    let repo = TempGitRepo::create("overlay-nested-git");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let nested = repo.path.join("src/vendor");
    fs::create_dir_all(&nested).expect("nested repository should be created");
    let nested_repo = TempGitRepo { path: nested };
    nested_repo.git(["init"]);
    nested_repo.git(["config", "user.email", "relay@example.invalid"]);
    nested_repo.git(["config", "user.name", "Relay Test"]);
    nested_repo.write("lib.rs", "fn nested_value() -> u32 { 0 }\n");
    nested_repo.git(["add", "."]);
    nested_repo.git(["commit", "-m", "initial"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay should not recurse into nested repositories");

    assert!(
        snapshot
            .files
            .iter()
            .all(|file| !file.path.starts_with("src/vendor/"))
    );
    assert!(snapshot.full_replace);
    assert!(!snapshot.resolved_commit_sha.starts_with("worktree:"));
}

#[test]
fn worktree_overlay_rejects_refs_that_are_not_checked_out() {
    let repo = TempGitRepo::create("overlay-ref-binding");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial = repo.git_text(["rev-parse", "HEAD"]);
    repo.write("src/lib.rs", "fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update"]);
    repo.write("src/lib.rs", "fn value() -> u32 { 2 }\n");
    let selector = CodeRepositorySelector::new("alias", initial, Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = build_index_snapshot(
        &repo.registration(),
        &selector,
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect_err("overlay ref should match checked-out HEAD");

    assert!(error.to_string().contains("worktree overlay ref"));
}

#[test]
fn worktree_overlay_records_rename_source_deletions() {
    let repo = TempGitRepo::create("overlay-rename");
    repo.write("src/old.rs", "fn old_name() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.git(["mv", "src/old.rs", "src/new.rs"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::WorktreeOverlay,
        Vec::new(),
    )
    .expect("overlay rename should index");

    assert_eq!(snapshot.deleted_paths, ["src/old.rs"]);
    assert!(snapshot.files.iter().any(|file| file.path == "src/new.rs"));
}
