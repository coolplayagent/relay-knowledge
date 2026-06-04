use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeRepositorySelector,
};

use super::test_fixtures::TempGitRepo;
use super::{build_index_snapshot, source_gitlink};

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
