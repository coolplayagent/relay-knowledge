use crate::domain::{CodeFileFingerprint, CodeIndexMode, CodeRepositoryRegistration};

use super::test_fixtures::TempGitRepo;
use super::{
    build_index_snapshot,
    changes::{TrackedEntryScope, tracked_entries_with_scope},
    git_ls_tree_full_scan_call_count_for_root, reset_git_ls_tree_full_scan_call_count_for_root,
    scope,
};

#[test]
fn submodule_child_scope_preserves_full_scope_when_parent_filter_covers_root() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        ".",
        vec!["src/module".to_owned(), "src/module/src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector =
        crate::domain::CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");

    let filters = scope::submodule_child_scope_filters("src/module", &registration, &selector)
        .expect("covered submodule should be expandable");

    assert!(filters.is_empty());
}

#[test]
fn empty_tracked_entry_scope_skips_git_tree_scan() {
    let repo = TempGitRepo::create("followup-empty-tracked-scope");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    reset_git_ls_tree_full_scan_call_count_for_root(repo.path.clone());

    let entries = tracked_entries_with_scope(&repo.path, "HEAD", &TrackedEntryScope::empty())
        .expect("empty scope should be a valid tracked entry scope");

    assert!(entries.is_empty());
    assert_eq!(git_ls_tree_full_scan_call_count_for_root(&repo.path), 0);
}

#[test]
fn incremental_gitlink_update_uses_discovered_source_root_scope() {
    let child = TempGitRepo::create("followup-discovered-root-child");
    child.write("lib.rs", "pub fn sdk_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("followup-discovered-root-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn old_sdk_value() -> u32 { 0 }\n",
    );
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        parent.path.display().to_string(),
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("registration should validate");
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    assert!(
        previous_hashes
            .iter()
            .any(|file| file.path == "external_deps/rust_sdk/lib.rs")
    );
    parent.git(["rm", "-r", "external_deps/rust_sdk"]);
    add_named_submodule(&parent, &child, "rust_sdk", "external_deps/rust_sdk");
    parent.git(["commit", "-m", "replace sdk directory with submodule"]);

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("discovered source-root gitlink update should expand selected children");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "external_deps/rust_sdk/lib.rs")
    );
}

fn add_named_submodule(parent: &TempGitRepo, child: &TempGitRepo, name: &str, path: &str) {
    let child_path = child.path.to_str().expect("child path should be unicode");
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--name",
        name,
        child_path,
        path,
    ]);
}

fn snapshot_fingerprints(
    snapshot: Result<crate::domain::CodeIndexSnapshot, super::CodeIndexError>,
) -> Vec<CodeFileFingerprint> {
    snapshot
        .expect("base snapshot should build")
        .files
        .into_iter()
        .map(|file| CodeFileFingerprint {
            path: file.path,
            blob_hash: file.blob_hash,
        })
        .collect()
}
