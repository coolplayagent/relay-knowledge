use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration,
    CodeRepositorySelector,
};
use std::fs;

use super::test_fixtures::TempGitRepo;
use super::{
    build_index_snapshot, changed_paths_for_diff_with_filters, deleted_symbol_names_for_diff,
    git_ls_tree_full_scan_call_count_for_root, reset_git_ls_tree_full_scan_call_count_for_root,
    source_gitlink,
};

#[test]
fn incremental_readded_submodule_deletes_historical_cached_children() {
    let old_child = TempGitRepo::create("review-readd-old-child");
    old_child.write("old.rs", "pub fn old_child_value() -> u32 { 1 }\n");
    old_child.git(["add", "."]);
    old_child.git(["commit", "-m", "old child"]);
    let new_child = TempGitRepo::create("review-readd-new-child");
    new_child.write("new.rs", "pub fn new_child_value() -> u32 { 1 }\n");
    new_child.git(["add", "."]);
    new_child.git(["commit", "-m", "new child"]);

    let parent = TempGitRepo::create("review-readd-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &old_child, "a_old", "src/module");
    parent.git(["commit", "-am", "add old submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let registration = parent.registration();
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove old submodule"]);
    add_named_submodule(&parent, &new_child, "z_new", "src/module");
    parent.git(["commit", "-am", "add new submodule"]);

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("re-added submodule should expand both cached and current sides");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/module/old.rs".to_owned())
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/module/new.rs")
    );
}

#[test]
fn incremental_gitlink_budget_counts_language_selected_children() {
    let child = TempGitRepo::create("review-language-budget-child");
    child.write("src/target.rs", "pub fn selected_value() -> u32 { 0 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-language-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &parent.registration(),
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    let submodule = TempGitRepo {
        path: parent.path.join("src/module"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    write_many_files(
        &submodule,
        "docs/generated",
        "md",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    submodule.write("src/target.rs", "pub fn selected_value() -> u32 { 1 }\n");
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "mixed language update"]);
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);

    let snapshot = build_index_snapshot(
        &parent.registration(),
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("non-rust submodule changes should not consume rust expansion budget");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/module/src/target.rs")
    );
}

#[test]
fn fallback_gitlink_expansion_enforces_combined_side_budget() {
    let old_child = TempGitRepo::create("review-budget-old-child");
    write_many_files(&old_child, "src/old", "rs", 260);
    old_child.git(["add", "."]);
    old_child.git(["commit", "-m", "old child"]);
    let new_child = TempGitRepo::create("review-budget-new-child");
    write_many_files(&new_child, "src/new", "rs", 260);
    new_child.git(["add", "."]);
    new_child.git(["commit", "-m", "new child"]);

    let parent = TempGitRepo::create("review-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &old_child, "a_old", "src/module");
    parent.git(["commit", "-am", "add old submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove old submodule"]);
    add_named_submodule(&parent, &new_child, "z_new", "src/module");
    parent.git(["commit", "-am", "add new submodule"]);

    let error = match source_gitlink::changed_gitlink_path_expansion(
        &parent.path,
        "src/module",
        &base,
        "HEAD",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH,
        &source_gitlink::GitlinkPathSelector::all(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("combined fallback expansion should enforce the shared budget"),
    };

    assert!(error.to_string().contains("expands to 520 files"));
}

#[test]
fn incremental_renamed_submodule_budget_counts_language_selected_children() {
    let child = TempGitRepo::create("review-rename-language-budget-child");
    child.write(
        "src/target.rs",
        "pub fn renamed_selected_value() -> u32 { 0 }\n",
    );
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-rename-language-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let submodule = TempGitRepo {
        path: parent.path.join("src/module"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    write_many_files(
        &submodule,
        "docs/generated",
        "md",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "large non-rust surface"]);
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &parent.registration(),
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    parent.git(["mv", "src/module", "src/renamed"]);
    parent.git(["commit", "-am", "rename submodule"]);

    let snapshot = build_index_snapshot(
        &parent.registration(),
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("non-rust renamed children should not consume rust expansion budget");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/renamed/src/target.rs")
    );
}

#[test]
fn impact_gitlink_budget_counts_language_selected_children() {
    let child = TempGitRepo::create("review-impact-language-budget-child");
    child.write(
        "src/target.rs",
        "pub fn impact_selected_value() -> u32 { 0 }\n",
    );
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-impact-language-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let submodule = TempGitRepo {
        path: parent.path.join("src/module"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    write_many_files(
        &submodule,
        "docs/generated",
        "md",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    submodule.write(
        "src/target.rs",
        "pub fn impact_selected_value() -> u32 { 1 }\n",
    );
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "mixed language update"]);
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);

    let paths =
        changed_paths_for_diff_with_filters(&parent.path, &base, "HEAD", &[], &["rust".to_owned()])
            .expect("non-rust impact paths should not consume rust expansion budget");

    assert!(paths.contains(&"src/module/src/target.rs".to_owned()));
}

#[test]
fn deleted_symbol_gitlink_budget_counts_language_selected_children() {
    let child = TempGitRepo::create("review-deleted-symbol-language-budget-child");
    child.write(
        "src/lib.rs",
        "pub fn removed_submodule_api() -> u32 { 0 }\n",
    );
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-deleted-symbol-language-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let submodule = TempGitRepo {
        path: parent.path.join("src/module"),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    write_many_files(
        &submodule,
        "docs/generated",
        "md",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "large non-rust surface"]);
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove submodule"]);
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");

    let names = deleted_symbol_names_for_diff(&parent.registration(), &selector, &base, "HEAD")
        .expect("non-rust deleted children should not consume rust expansion budget");

    assert!(names.contains(&"removed_submodule_api".to_owned()));
}

#[test]
fn impact_fallback_gitlink_expansion_enforces_combined_side_budget() {
    let old_child = TempGitRepo::create("review-impact-budget-old-child");
    write_many_files(&old_child, "src/old", "rs", 260);
    old_child.git(["add", "."]);
    old_child.git(["commit", "-m", "old child"]);
    let new_child = TempGitRepo::create("review-impact-budget-new-child");
    write_many_files(&new_child, "src/new", "rs", 260);
    new_child.git(["add", "."]);
    new_child.git(["commit", "-m", "new child"]);
    let parent = TempGitRepo::create("review-impact-budget-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &old_child, "a_old", "src/module");
    parent.git(["commit", "-am", "add old submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove old submodule"]);
    add_named_submodule(&parent, &new_child, "z_new", "src/module");
    parent.git(["commit", "-am", "add new submodule"]);

    let error = changed_paths_for_diff_with_filters(&parent.path, &base, "HEAD", &[], &[])
        .expect_err("impact fallback should enforce the shared expansion budget");

    assert!(error.to_string().contains("expands to 520 files"));
}

#[test]
fn impact_missing_submodule_does_not_scan_unrelated_cached_gitdir() {
    let child = TempGitRepo::create("review-unrelated-cache-child");
    child.write("private.rs", "pub fn private_cached_value() -> u32 { 0 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "private child"]);
    let child_commit = child.git_text(["rev-parse", "HEAD"]);
    let parent = TempGitRepo::create("review-unrelated-cache-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "private_cache", "vendor/private");
    parent.git(["commit", "-am", "add unrelated configured submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    parent.git([
        "update-index",
        "--add",
        "--cacheinfo",
        "160000",
        &child_commit,
        "src/missing",
    ]);
    parent.git(["commit", "-m", "add unconfigured gitlink"]);

    let paths = changed_paths_for_diff_with_filters(&parent.path, &base, "HEAD", &[], &[])
        .expect("unconfigured gitlink should remain unresolved");

    assert!(paths.contains(&"src/missing".to_owned()));
    assert!(!paths.contains(&"src/missing/private.rs".to_owned()));
}

#[test]
fn impact_fallback_gitlink_expansion_uses_child_scope_before_ls_tree() {
    let old_child = TempGitRepo::create("review-scoped-fallback-old-child");
    old_child.write("old.rs", "pub fn old_child_value() -> u32 { 0 }\n");
    old_child.git(["add", "."]);
    old_child.git(["commit", "-m", "old child"]);
    let new_child = TempGitRepo::create("review-scoped-fallback-new-child");
    new_child.write(
        "src/target.rs",
        "pub fn selected_child_value() -> u32 { 1 }\n",
    );
    write_many_files(
        &new_child,
        "noise/generated",
        "rs",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    new_child.git(["add", "."]);
    new_child.git(["commit", "-m", "new child"]);

    let parent = TempGitRepo::create("review-scoped-fallback-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &old_child, "a_old", "src/module");
    parent.git(["commit", "-am", "add old submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove old submodule"]);
    add_named_submodule(&parent, &new_child, "z_new", "src/module");
    parent.git(["commit", "-am", "add new submodule"]);
    let submodule_root = parent.path.join("src/module");
    reset_git_ls_tree_full_scan_call_count_for_root(submodule_root.clone());

    let paths = changed_paths_for_diff_with_filters(
        &parent.path,
        &base,
        "HEAD",
        &["src/module/src/target.rs".to_owned()],
        &[],
    )
    .expect("path-scoped fallback should not materialize every submodule child");

    assert!(paths.contains(&"src/module/src/target.rs".to_owned()));
    assert_eq!(
        git_ls_tree_full_scan_call_count_for_root(&submodule_root),
        0
    );
}

#[test]
fn incremental_renamed_empty_submodule_is_handled_without_blob_fallback() {
    let child = TempGitRepo::create("review-empty-rename-child");
    child.git(["commit", "--allow-empty", "-m", "empty child"]);
    let parent = TempGitRepo::create("review-empty-rename-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "empty", "src/module");
    parent.git(["commit", "-am", "add empty submodule"]);
    let base = parent.git_text(["rev-parse", "HEAD"]);
    let registration = parent.registration();
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    parent.git(["mv", "src/module", "src/renamed"]);
    parent.git(["commit", "-am", "rename empty submodule"]);

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("empty renamed gitlink should not be parsed as a regular blob");

    assert!(
        snapshot
            .files
            .iter()
            .all(|file| !file.path.starts_with("src/renamed/"))
    );
}

#[test]
fn worktree_overlay_marks_staged_submodule_removal_with_no_selected_children() {
    let child = TempGitRepo::create("review-empty-staged-remove-child");
    child.write("src/lib.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-empty-staged-remove-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let registration = scoped_registration(&parent, vec!["src/module/tests".to_owned()]);
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    parent.git(["rm", "--cached", "-f", "src/module"]);

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        previous_hashes,
    )
    .expect("empty staged removal should still mark the overlay dirty");

    assert!(snapshot.files.is_empty());
    assert!(snapshot.resolved_commit_sha.starts_with("worktree:"));
}

#[test]
fn impact_nested_cached_submodule_uses_parent_gitdir_context() {
    let nested = TempGitRepo::create("review-nested-cache-nested");
    nested.write("src/nested.rs", "pub fn nested_value() -> u32 { 1 }\n");
    nested.git(["add", "."]);
    nested.git(["commit", "-m", "nested"]);
    let child = TempGitRepo::create("review-nested-cache-child");
    child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    add_named_submodule(&child, &nested, "nested", "nested");
    child.git(["commit", "-am", "add nested submodule"]);
    let parent = TempGitRepo::create("review-nested-cache-parent");
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
        path: parent.path.join("vendor/module/nested"),
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
    child_checkout.git(["add", "nested"]);
    child_checkout.git(["commit", "-m", "update nested pointer"]);
    parent.git(["add", "vendor/module"]);
    parent.git(["commit", "-m", "update outer pointer"]);
    parent.git(["submodule", "deinit", "-f", "vendor/module"]);

    let paths = changed_paths_for_diff_with_filters(&parent.path, &base, "HEAD", &[], &[])
        .expect("nested cached gitdir should expand changed child paths");

    assert!(paths.contains(&"vendor/module/nested/src/nested.rs".to_owned()));
    assert!(!paths.contains(&"vendor/module/nested".to_owned()));
}

#[test]
fn incremental_deleted_unavailable_gitlink_deletes_previous_children() {
    let child = TempGitRepo::create("review-delete-unavailable-child");
    child.write("src/lib.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-delete-unavailable-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
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
    parent.git(["rm", "-f", "src/module"]);
    parent.git(["commit", "-m", "remove submodule"]);
    remove_submodule_checkout_and_gitdir(&parent, "src/module");

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("unavailable removed gitlink should delete prior indexed children");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/module/src/lib.rs".to_owned())
    );
}

#[test]
fn full_snapshot_scoped_submodule_child_uses_pathspec_after_recursion() {
    let child = TempGitRepo::create("review-full-pathspec-child");
    child.write(
        "src/target.rs",
        "pub fn selected_child_value() -> u32 { 1 }\n",
    );
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let parent = TempGitRepo::create("review-full-pathspec-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "vendor/module");
    parent.git(["commit", "-am", "add submodule"]);
    let submodule = checked_out_submodule(&parent, "vendor/module");
    write_many_files(
        &submodule,
        "noise/generated",
        "rs",
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH + 1,
    );
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "large unselected surface"]);
    parent.git(["add", "vendor/module"]);
    parent.git(["commit", "-m", "update submodule noise"]);
    let submodule_root = parent.path.join("vendor/module");
    reset_git_ls_tree_full_scan_call_count_for_root(submodule_root.clone());
    let registration = scoped_registration(&parent, vec!["vendor/module/src/target.rs".to_owned()]);

    let snapshot = build_index_snapshot(
        &registration,
        &parent.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("scoped full snapshot should use child pathspecs inside submodules");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "vendor/module/src/target.rs")
    );
    assert_eq!(
        git_ls_tree_full_scan_call_count_for_root(&submodule_root),
        0
    );
}

#[test]
fn full_snapshot_scoped_nested_submodule_pathspec_enters_gitlink() {
    let nested = TempGitRepo::create("review-full-nested-pathspec-nested");
    nested.write("src/nested.rs", "pub fn nested_value() -> u32 { 1 }\n");
    nested.git(["add", "."]);
    nested.git(["commit", "-m", "nested"]);
    let child = TempGitRepo::create("review-full-nested-pathspec-child");
    child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    add_named_submodule(&child, &nested, "nested", "nested");
    child.git(["commit", "-am", "add nested submodule"]);
    let parent = TempGitRepo::create("review-full-nested-pathspec-parent");
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
    let registration = scoped_registration(
        &parent,
        vec!["vendor/module/nested/src/nested.rs".to_owned()],
    );

    let snapshot = build_index_snapshot(
        &registration,
        &parent.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("scoped nested submodule pathspec should still discover the nested gitlink");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "vendor/module/nested/src/nested.rs")
    );
}

#[test]
fn incremental_changed_unavailable_gitlink_deletes_previous_children() {
    let child = TempGitRepo::create("review-changed-unavailable-child");
    child.write("src/lib.rs", "pub fn old_child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "old child"]);
    let parent = TempGitRepo::create("review-changed-unavailable-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
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
    let submodule = checked_out_submodule(&parent, "src/module");
    fs::remove_file(parent.path.join("src/module/src/lib.rs"))
        .expect("old child file should be removable");
    submodule.write("src/new.rs", "pub fn new_child_value() -> u32 { 2 }\n");
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "new child"]);
    parent.git(["add", "src/module"]);
    parent.git(["commit", "-m", "update submodule"]);
    remove_submodule_checkout_and_gitdir(&parent, "src/module");

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("unavailable changed gitlink should delete prior indexed children");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/module/src/lib.rs".to_owned())
    );
}

#[test]
fn worktree_overlay_changed_unavailable_gitlink_deletes_previous_children() {
    let child = TempGitRepo::create("review-overlay-changed-unavailable-child");
    child.write("src/lib.rs", "pub fn old_child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "old child"]);
    let parent = TempGitRepo::create("review-overlay-changed-unavailable-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let registration = parent.registration();
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    let submodule = checked_out_submodule(&parent, "src/module");
    fs::remove_file(parent.path.join("src/module/src/lib.rs"))
        .expect("old child file should be removable");
    submodule.write("src/new.rs", "pub fn new_child_value() -> u32 { 2 }\n");
    submodule.git(["add", "."]);
    submodule.git(["commit", "-m", "new child"]);
    parent.git(["add", "src/module"]);
    remove_submodule_checkout_and_gitdir(&parent, "src/module");

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        previous_hashes,
    )
    .expect("unavailable overlay update should delete prior indexed children");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/module/src/lib.rs".to_owned())
    );
}

#[test]
fn worktree_overlay_removed_unavailable_gitlink_deletes_previous_children() {
    let child = TempGitRepo::create("review-overlay-removed-unavailable-child");
    child.write("src/lib.rs", "pub fn old_child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "old child"]);
    let parent = TempGitRepo::create("review-overlay-removed-unavailable-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    add_named_submodule(&parent, &child, "module", "src/module");
    parent.git(["commit", "-am", "add submodule"]);
    let registration = parent.registration();
    let selector = parent.selector();
    let previous_hashes = snapshot_fingerprints(build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
    ));
    parent.git(["rm", "-f", "src/module"]);
    remove_submodule_checkout_and_gitdir(&parent, "src/module");

    let snapshot = build_index_snapshot(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        previous_hashes,
    )
    .expect("unavailable overlay removal should delete prior indexed children");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/module/src/lib.rs".to_owned())
    );
}

#[test]
fn full_snapshot_overlapping_nested_submodule_filters_do_not_duplicate_children() {
    let nested = TempGitRepo::create("review-dedupe-nested-pathspec-nested");
    nested.write("src/nested.rs", "pub fn nested_value() -> u32 { 1 }\n");
    nested.git(["add", "."]);
    nested.git(["commit", "-m", "nested"]);
    let child = TempGitRepo::create("review-dedupe-nested-pathspec-child");
    child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    add_named_submodule(&child, &nested, "nested", "nested");
    child.git(["commit", "-am", "add nested submodule"]);
    let parent = TempGitRepo::create("review-dedupe-nested-pathspec-parent");
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
    let registration = scoped_registration(
        &parent,
        vec![
            "vendor/module/nested".to_owned(),
            "vendor/module/nested/src/nested.rs".to_owned(),
        ],
    );

    let snapshot = build_index_snapshot(
        &registration,
        &parent.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("overlapping nested submodule filters should not duplicate expansion");
    let nested_file_count = snapshot
        .files
        .iter()
        .filter(|file| file.path == "vendor/module/nested/src/nested.rs")
        .count();

    assert_eq!(nested_file_count, 1);
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

fn scoped_registration(
    parent: &TempGitRepo,
    path_filters: Vec<String>,
) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new(
        "repo",
        "alias",
        parent.path.display().to_string(),
        path_filters,
        Vec::new(),
    )
    .expect("registration should validate")
}

fn checked_out_submodule(parent: &TempGitRepo, path: &str) -> TempGitRepo {
    let submodule = TempGitRepo {
        path: parent.path.join(path),
    };
    submodule.git(["config", "user.email", "relay@example.invalid"]);
    submodule.git(["config", "user.name", "Relay Test"]);
    submodule
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

fn write_many_files(repo: &TempGitRepo, prefix: &str, extension: &str, count: usize) {
    for index in 0..count {
        repo.write(
            &format!("{prefix}_{index}.{extension}"),
            &format!("pub fn generated_{index}() -> u32 {{ {index} }}\n"),
        );
    }
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
