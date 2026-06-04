use crate::domain::{CodeFileFingerprint, CodeIndexMode, CodeRepositoryRegistration};

use super::test_fixtures::TempGitRepo;
use super::{
    build_index_snapshot, changed_paths_for_diff_with_filters,
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

#[test]
fn full_snapshot_scoped_nested_submodule_probe_uses_gitlink_boundary() {
    let nested = TempGitRepo::create("followup-boundary-nested");
    nested.write("src/nested.rs", "pub fn nested_value() -> u32 { 1 }\n");
    nested.git(["add", "."]);
    nested.git(["commit", "-m", "nested"]);
    let child = TempGitRepo::create("followup-boundary-child");
    child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    add_named_submodule(&child, &nested, "deps_nested", "deps/nested");
    child.git(["commit", "-am", "add nested submodule"]);
    let parent = TempGitRepo::create("followup-boundary-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "vendor/module");
    parent.git(["commit", "-am", "add outer submodule"]);
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "update",
        "--init",
        "--recursive",
        "vendor/module",
    ]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        parent.path.display().to_string(),
        vec!["vendor/module/deps/nested/src/nested.rs".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");

    let snapshot = build_index_snapshot(
        &registration,
        &parent.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("scoped pathspec should probe the actual nested gitlink path");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "vendor/module/deps/nested/src/nested.rs")
    );
}

#[test]
fn impact_nested_deinitialized_child_uses_initialized_parent_gitdir() {
    let nested = TempGitRepo::create("followup-nested-deinit-nested");
    nested.write("src/nested.rs", "pub fn nested_value() -> u32 { 1 }\n");
    nested.git(["add", "."]);
    nested.git(["commit", "-m", "nested"]);
    let child = TempGitRepo::create("followup-nested-deinit-child");
    child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    add_named_submodule(&child, &nested, "deps_nested", "deps/nested");
    child.git(["commit", "-am", "add nested submodule"]);
    let parent = TempGitRepo::create("followup-nested-deinit-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "vendor/module");
    parent.git(["commit", "-am", "add outer submodule"]);
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "update",
        "--init",
        "--recursive",
        "vendor/module",
    ]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let nested_checkout = TempGitRepo {
        path: parent.path.join("vendor/module/deps/nested"),
    };
    nested_checkout.git(["config", "user.email", "relay@example.invalid"]);
    nested_checkout.git(["config", "user.name", "Relay Test"]);
    nested_checkout.write("src/nested.rs", "pub fn nested_value() -> u32 { 2 }\n");
    nested_checkout.git(["add", "."]);
    nested_checkout.git(["commit", "-m", "nested update"]);
    let child_checkout = TempGitRepo {
        path: parent.path.join("vendor/module"),
    };
    child_checkout.git(["config", "user.email", "relay@example.invalid"]);
    child_checkout.git(["config", "user.name", "Relay Test"]);
    child_checkout.git(["add", "deps/nested"]);
    child_checkout.git(["commit", "-m", "update nested pointer"]);
    parent.git(["add", "vendor/module"]);
    parent.git(["commit", "-m", "update outer pointer"]);
    child_checkout.git(["submodule", "deinit", "-f", "deps/nested"]);

    let paths = changed_paths_for_diff_with_filters(&parent.path, &base, "HEAD", &[], &[])
        .expect("initialized parent gitdir should resolve deinitialized nested cached objects");

    assert!(paths.contains(&"vendor/module/deps/nested/src/nested.rs".to_owned()));
    assert!(!paths.contains(&"vendor/module/deps/nested".to_owned()));
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
