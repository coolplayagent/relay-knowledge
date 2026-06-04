use crate::domain::{CodeFileFingerprint, CodeIndexMode};
use std::{fs, process::Command};

use super::{
    build_index_snapshot, changed_paths_for_diff,
    changes::{TrackedEntryScope, tracked_entries_with_scope},
    resolve_repository_snapshot_with_path_filters, source_gitlink,
    test_fixtures::TempGitRepo,
    tracked_entries,
};

#[test]
fn incremental_submodule_update_stores_submodule_aware_tree_hash() {
    let child = TempGitRepo::create("incremental-hash-child");
    child.write("lib.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("incremental-hash-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let registration = parent.registration();
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));

    commit_submodule_file(
        &parent,
        "src/module",
        "lib.rs",
        "pub fn child_value() -> u32 { 2 }\n",
    );
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("incremental submodule update should build");
    let expected =
        resolve_repository_snapshot_with_path_filters(&parent.path, "HEAD", &["src".to_owned()])
            .expect("freshness snapshot should resolve");

    assert_eq!(snapshot.tree_hash, expected.1);
}

#[test]
fn impact_type_change_from_submodule_to_file_includes_regular_path() {
    let child = TempGitRepo::create("impact-typechange-child");
    child.write("lib.rs", "pub fn child_plugin() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("impact-typechange-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "src/plugin.rs");
    parent.git(["commit", "-am", "base submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);

    parent.git(["rm", "-f", "src/plugin.rs"]);
    parent.write("src/plugin.rs", "pub fn new_plugin() -> u32 { 2 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "replace submodule with file"]);

    let paths = changed_paths_for_diff(&parent.path, &base, "HEAD")
        .expect("impact paths should include both sides of type change");

    assert!(paths.contains(&"src/plugin.rs".to_owned()));
    assert!(paths.contains(&"src/plugin.rs/lib.rs".to_owned()));
}

#[test]
fn scoped_submodule_diff_bounds_after_child_filtering() {
    let child = TempGitRepo::create("diff-scoped-bound-child");
    child.write("noise/one.rs", "pub fn noise_one() -> u32 { 0 }\n");
    child.write("noise/two.rs", "pub fn noise_two() -> u32 { 0 }\n");
    child.write("src/target.rs", "pub fn scoped_target() -> u32 { 0 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("diff-scoped-bound-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "vendor/module");
    parent.git(["commit", "-am", "add submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);

    let submodule = TempGitRepo {
        path: parent.path.join("vendor/module"),
    };
    git_in(
        &submodule.path,
        ["config", "user.email", "relay@example.invalid"],
    );
    git_in(&submodule.path, ["config", "user.name", "Relay Test"]);
    submodule.write("noise/one.rs", "pub fn noise_one() -> u32 { 1 }\n");
    submodule.write("noise/two.rs", "pub fn noise_two() -> u32 { 1 }\n");
    submodule.write("src/target.rs", "pub fn scoped_target() -> u32 { 1 }\n");
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "large child update"]);
    parent.git(["add", "vendor/module"]);
    parent.git(["commit", "-m", "update submodule"]);

    let selector = source_gitlink::GitlinkPathSelector::new(
        &|path| path == "vendor/module/src/target.rs",
        &|path| {
            path == "vendor/module"
                || path == "vendor/module/src/target.rs"
                || "vendor/module/src/target.rs".starts_with(&format!("{path}/"))
        },
    );
    let expansion = source_gitlink::changed_gitlink_path_expansion(
        &parent.path,
        "vendor/module",
        &base,
        "HEAD",
        2,
        &selector,
    )
    .expect("scoped submodule diff should not count unrelated child paths")
    .expect("submodule update should expand");

    assert_eq!(expansion.head_paths.len(), 1);
    assert!(expansion.head_paths.contains("vendor/module/src/target.rs"));
}

#[test]
fn impact_submodule_update_without_cached_objects_keeps_gitlink_path() {
    let child = TempGitRepo::create("impact-unavailable-child");
    child.write("lib.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("impact-unavailable-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "vendor/module");
    parent.git(["commit", "-am", "add submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);

    commit_submodule_file(
        &parent,
        "vendor/module",
        "lib.rs",
        "pub fn child_value() -> u32 { 2 }\n",
    );
    parent.git(["add", "vendor/module"]);
    parent.git(["commit", "-m", "update submodule"]);
    remove_submodule_checkout_and_gitdir(&parent, "vendor/module");

    let paths = changed_paths_for_diff(&parent.path, &base, "HEAD")
        .expect("unavailable submodule updates should not fail impact paths");

    assert_eq!(paths, vec!["vendor/module".to_owned()]);
}

#[test]
fn config_submodule_name_cannot_escape_git_modules() {
    let child = TempGitRepo::create("malicious-name-child");
    child.write("lib.rs", "pub fn outside_child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("malicious-name-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "vendor/module");
    parent.git(["commit", "-am", "add submodule"]);
    let outside_name = format!(
        "{}-outside",
        parent
            .path
            .file_name()
            .expect("parent temp path should have a file name")
            .to_string_lossy()
    );
    let outside_git_dir = parent
        .path
        .parent()
        .expect("parent temp path should have a parent")
        .join(&outside_name);
    git_clone_bare(&child, &outside_git_dir);
    parent.git(["submodule", "deinit", "-f", "vendor/module"]);
    remove_submodule_checkout_and_gitdir(&parent, "vendor/module");
    parent.write(
        ".gitmodules",
        &format!(
            "[submodule \"../../../{outside_name}\"]\n\tpath = vendor/module\n\turl = ignored\n"
        ),
    );
    parent.git(["add", ".gitmodules"]);
    parent.git(["commit", "-m", "malicious submodule name"]);

    let entries = tracked_entries(&parent.path, &parent.git_text(["rev-parse", "HEAD"]))
        .expect("malicious submodule names should be treated as unavailable");

    assert!(
        entries
            .iter()
            .all(|entry| entry.path != "vendor/module/lib.rs")
    );
    let _ = fs::remove_dir_all(outside_git_dir);
}

#[test]
fn tracked_entry_scope_filters_blobs_inside_scoped_submodule() {
    let child = TempGitRepo::create("tracked-scope-child");
    child.write("noise/ignored.rs", "pub fn ignored_noise() -> u32 { 0 }\n");
    child.write("src/target.rs", "pub fn scoped_target() -> u32 { 0 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("tracked-scope-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_submodule(&parent, &child, "vendor/module");
    parent.git(["commit", "-am", "add submodule"]);
    let filters = ["vendor/module/src/target.rs".to_owned()];
    let scope = TrackedEntryScope::from_path_filters(filters.iter());

    let entries = tracked_entries_with_scope(&parent.path, "HEAD", &scope)
        .expect("scoped tracked entries should build");

    assert!(entries.iter().any(|entry| entry.path == "src/lib.rs"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "vendor/module/src/target.rs")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.path != "vendor/module/noise/ignored.rs")
    );
}

fn snapshot_fingerprints(
    snapshot: Result<crate::domain::CodeIndexSnapshot, super::CodeIndexError>,
) -> Vec<CodeFileFingerprint> {
    snapshot
        .expect("snapshot should build")
        .files
        .into_iter()
        .map(|file| CodeFileFingerprint {
            path: file.path,
            blob_hash: file.blob_hash,
        })
        .collect()
}

fn add_submodule(parent: &TempGitRepo, child: &TempGitRepo, path: &str) {
    let child_path = child
        .path
        .to_str()
        .expect("child path should be unicode")
        .to_owned();
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        child_path.as_str(),
        path,
    ]);
}

fn commit_submodule_file(parent: &TempGitRepo, path: &str, file: &str, content: &str) {
    let submodule_path = parent.path.join(path);
    git_in(
        &submodule_path,
        ["config", "user.email", "relay@example.invalid"],
    );
    git_in(&submodule_path, ["config", "user.name", "Relay Test"]);
    fs::write(submodule_path.join(file), content).expect("submodule file should be written");
    git_in(&submodule_path, ["add", "."]);
    git_in(&submodule_path, ["commit", "-m", "submodule update"]);
}

fn remove_submodule_checkout_and_gitdir(parent: &TempGitRepo, path: &str) {
    let git_dir = parent.path.join(".git/modules").join(path);
    if git_dir.exists() {
        fs::remove_dir_all(git_dir).expect("submodule gitdir should be removable");
    }
    let worktree = parent.path.join(path);
    if worktree.exists() {
        fs::remove_dir_all(worktree).expect("submodule worktree should be removable");
    }
}

fn git_clone_bare(source: &TempGitRepo, destination: &std::path::Path) {
    let source_path = source
        .path
        .to_str()
        .expect("source path should be unicode")
        .to_owned();
    let destination_path = destination
        .to_str()
        .expect("destination path should be unicode")
        .to_owned();
    let output = Command::new("git")
        .args(["clone", "--bare", &source_path, &destination_path])
        .output()
        .expect("git clone should run");
    assert!(
        output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_in<const N: usize>(path: &std::path::Path, args: [&str; N]) {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
