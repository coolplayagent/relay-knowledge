use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration,
    CodeRepositorySelector,
};
use std::fs;

use super::{
    RepositorySourceKind, SourceGrepKind, SourceGrepRequest, build_index_snapshot,
    changed_paths_for_diff, deleted_symbol_names_for_diff, git_show_call_count_for_root,
    prepare_full_index_plan, reset_git_show_call_count_for_root,
    reset_tracked_entries_call_count_for_root, resolve_repository_snapshot_with_path_filters,
    source_grep_matches, source_snapshot_batch_bytes, test_fixtures::TempGitRepo, tracked_entries,
    tracked_entries_call_count_for_root,
};

#[test]
fn tracked_entries_expand_initialized_submodule_files() {
    let fixture = SubmoduleFixture::create("tracked-entries");

    let entries = tracked_entries(&fixture.parent.path, &fixture.parent_head())
        .expect("tracked entries should expand submodules");

    assert!(entries.iter().any(|entry| entry.path == "src/lib.rs"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "vendor/module/src/child.rs")
    );
}

#[test]
fn tracked_entries_expand_deinitialized_submodule_files_from_gitdir() {
    let fixture = SubmoduleFixture::create("tracked-deinit");
    fixture
        .parent
        .git(["submodule", "deinit", "-f", "vendor/module"]);

    let entries = tracked_entries(&fixture.parent.path, &fixture.parent_head())
        .expect("tracked entries should expand deinitialized submodules from gitdir");

    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "vendor/module/src/child.rs")
    );
}

#[test]
fn tracked_entries_honor_custom_submodule_gitdir_names() {
    let fixture = SubmoduleFixture::create_with_submodule_name("tracked-custom-name", "custom");
    fixture
        .parent
        .git(["submodule", "deinit", "-f", "vendor/module"]);

    let entries = tracked_entries(&fixture.parent.path, &fixture.parent_head())
        .expect("tracked entries should resolve custom submodule gitdir names");

    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "vendor/module/src/child.rs")
    );
}

#[test]
fn full_index_plan_parses_submodule_files_under_parent_paths() {
    let fixture = SubmoduleFixture::create("full-index");
    let mut plan = prepare_full_index_plan(
        fixture.parent_registration(),
        fixture.parent_selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("full index plan should include submodule files");
    let mut indexed_paths = Vec::new();
    loop {
        let (next_plan, batch) = plan
            .parse_next_batch()
            .expect("submodule batch should parse");
        plan = next_plan;
        let Some(batch) = batch else {
            break;
        };
        indexed_paths.extend(batch.files.into_iter().map(|file| file.path));
    }

    assert!(indexed_paths.contains(&"vendor/module/src/child.rs".to_owned()));
}

#[test]
fn source_batch_keeps_parent_blobs_batched_around_submodule_paths() {
    let fixture = SubmoduleFixture::create("source-batch");
    let commit = fixture.parent_head();
    let paths = vec![
        "src/lib.rs".to_owned(),
        "vendor/module/src/child.rs".to_owned(),
    ];
    reset_git_show_call_count_for_root(fixture.parent.path.clone());

    let blobs = source_snapshot_batch_bytes(
        &fixture.parent.path,
        RepositorySourceKind::Git,
        &commit,
        &paths,
    )
    .expect("mixed parent/submodule batch should load");

    assert_eq!(blobs.len(), 2);
    assert_eq!(
        String::from_utf8_lossy(&blobs[0]),
        "pub fn parent_value() -> u32 { 1 }\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&blobs[1]),
        "pub fn child_value() -> u32 { 1 }\n"
    );
    assert_eq!(git_show_call_count_for_root(&fixture.parent.path), 1);
}

#[test]
fn source_grep_reads_submodule_materialized_source() {
    let fixture = SubmoduleFixture::create("source-grep");

    let outcome = source_grep_matches(
        &fixture.parent_registration(),
        &fixture.parent_head(),
        SourceGrepRequest {
            query: "child_value".to_owned(),
            paths: vec!["vendor/module/src/child.rs".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source grep should read submodule blobs");

    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "vendor/module/src/child.rs");
}

#[test]
fn full_index_snapshot_hash_changes_when_submodule_becomes_available() {
    let fixture = SubmoduleFixture::create("freshness");
    fixture
        .parent
        .git(["submodule", "deinit", "-f", "vendor/module"]);
    let git_dir = fixture.parent.path.join(".git/modules/vendor/module");
    if git_dir.exists() {
        fs::remove_dir_all(&git_dir).expect("submodule gitdir should be removable");
    }
    let worktree = fixture.parent.path.join("vendor/module");
    if worktree.exists() {
        fs::remove_dir_all(&worktree).expect("submodule worktree should be removable");
    }
    let path_filters = vec![".".to_owned()];

    let before =
        resolve_repository_snapshot_with_path_filters(&fixture.parent.path, "HEAD", &path_filters)
            .expect("unavailable submodule snapshot should resolve");
    fixture.parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "update",
        "--init",
        "vendor/module",
    ]);
    let after =
        resolve_repository_snapshot_with_path_filters(&fixture.parent.path, "HEAD", &path_filters)
            .expect("available submodule snapshot should resolve");

    assert_eq!(before.0, after.0);
    assert_ne!(before.1, after.1);
}

#[test]
fn incremental_index_expands_submodule_commit_updates() {
    let fixture = SubmoduleFixture::create("incremental");
    let base = fixture.parent_head();
    let base_snapshot = build_index_snapshot(
        &fixture.parent_registration(),
        &fixture.parent_selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("base full snapshot should build");
    let previous_hashes = base_snapshot
        .files
        .into_iter()
        .map(|file| CodeFileFingerprint {
            path: file.path,
            blob_hash: file.blob_hash,
        })
        .collect::<Vec<_>>();
    fixture.update_submodule_file("pub fn child_value() -> u32 { 2 }\n");

    let snapshot = build_index_snapshot(
        &fixture.parent_registration(),
        &fixture.parent_selector(),
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("incremental snapshot should expand gitlink updates");

    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "vendor/module/src/child.rs")
    );
}

#[test]
fn incremental_submodule_deletion_deletes_expanded_base_child_paths() {
    let fixture = SubmoduleFixture::create("incremental-delete");
    let base = fixture.parent_head();
    let previous_hashes = fixture.previous_hashes();
    fixture.parent.git(["rm", "-f", "vendor/module"]);
    fixture.parent.git(["commit", "-m", "remove submodule"]);

    let snapshot = build_index_snapshot(
        &fixture.parent_registration(),
        &fixture.parent_selector(),
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("submodule deletion should expand base child paths");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"vendor/module/src/child.rs".to_owned())
    );
    assert!(
        snapshot
            .deleted_paths
            .contains(&"vendor/module/src/unchanged.rs".to_owned())
    );
    assert!(!snapshot.deleted_paths.contains(&"vendor/module".to_owned()));
}

#[test]
fn impact_diff_paths_expand_submodule_commit_updates() {
    let fixture = SubmoduleFixture::create("impact");
    let base = fixture.parent_head();
    fixture.update_submodule_file("pub fn child_value() -> u32 { 3 }\n");

    let paths = changed_paths_for_diff(&fixture.parent.path, &base, "HEAD")
        .expect("impact paths should expand gitlink updates");

    assert!(paths.contains(&"vendor/module/src/child.rs".to_owned()));
    assert!(!paths.contains(&"vendor/module".to_owned()));
}

#[test]
fn impact_diff_paths_only_include_changed_submodule_files() {
    let fixture = SubmoduleFixture::create("impact-nested-diff");
    let base = fixture.parent_head();
    fixture.update_submodule_file("pub fn child_value() -> u32 { 4 }\n");

    let paths = changed_paths_for_diff(&fixture.parent.path, &base, "HEAD")
        .expect("impact paths should use the nested submodule diff");

    assert!(paths.contains(&"vendor/module/src/child.rs".to_owned()));
    assert!(!paths.contains(&"vendor/module/src/unchanged.rs".to_owned()));
}

#[test]
fn impact_diff_paths_do_not_expand_trees_for_ordinary_file_changes() {
    let repo = TempGitRepo::create("impact-ordinary");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "head"]);
    reset_tracked_entries_call_count_for_root(repo.path.clone());

    let paths = changed_paths_for_diff(&repo.path, &base, "HEAD")
        .expect("ordinary impact diff should succeed");

    assert_eq!(paths, vec!["src/lib.rs".to_owned()]);
    assert_eq!(tracked_entries_call_count_for_root(&repo.path), 0);
}

#[test]
fn deleted_symbol_names_include_symbols_removed_by_submodule_update() {
    let fixture = SubmoduleFixture::create("deleted-symbols");
    fixture.update_submodule_file(
        "pub fn child_value() -> u32 { 1 }\npub fn removed_api() -> u32 { 1 }\n",
    );
    let base = fixture.parent_head();
    fixture.update_submodule_file("pub fn child_value() -> u32 { 2 }\n");

    let names = deleted_symbol_names_for_diff(
        &fixture.parent_registration(),
        &fixture.parent_selector(),
        &base,
        "HEAD",
    )
    .expect("deleted symbol extraction should read submodule blobs");

    assert!(names.contains(&"removed_api".to_owned()));
    assert!(!names.contains(&"child_value".to_owned()));
}

#[test]
fn full_index_does_not_expand_submodules_outside_path_scope() {
    let fixture = SubmoduleFixture::create("scoped-full-index");
    let submodule_root = fixture.parent.path.join("vendor/module");
    reset_tracked_entries_call_count_for_root(submodule_root.clone());

    let mut plan = prepare_full_index_plan(
        fixture.parent_src_registration(),
        fixture.parent_selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("scoped full index plan should build");
    let mut indexed_paths = Vec::new();
    loop {
        let (next_plan, batch) = plan.parse_next_batch().expect("batch should parse");
        plan = next_plan;
        let Some(batch) = batch else {
            break;
        };
        indexed_paths.extend(batch.files.into_iter().map(|file| file.path));
    }

    assert!(indexed_paths.contains(&"src/lib.rs".to_owned()));
    assert!(
        indexed_paths
            .iter()
            .all(|path| !path.starts_with("vendor/module/"))
    );
    assert_eq!(tracked_entries_call_count_for_root(&submodule_root), 0);
}

#[test]
fn incremental_type_change_from_file_to_submodule_deletes_regular_file() {
    let fixture = TypeChangeFixture::regular_file("file-to-submodule");
    let base = fixture.parent_head();
    let previous_hashes = fixture.previous_hashes();
    fixture.replace_file_with_submodule();

    let snapshot = build_index_snapshot(
        &fixture.parent.registration(),
        &fixture.parent.selector(),
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("file to submodule type change should index");

    assert!(snapshot.deleted_paths.contains(&"src/plugin.rs".to_owned()));
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/plugin.rs/lib.rs")
    );
}

#[test]
fn incremental_type_change_from_submodule_to_file_parses_regular_file() {
    let fixture = TypeChangeFixture::submodule("submodule-to-file");
    let base = fixture.parent_head();
    let previous_hashes = fixture.previous_hashes();
    fixture.replace_submodule_with_file();

    let snapshot = build_index_snapshot(
        &fixture.parent.registration(),
        &fixture.parent.selector(),
        CodeIndexMode::Incremental {
            base_ref: base,
            head_ref: "HEAD".to_owned(),
        },
        previous_hashes,
    )
    .expect("submodule to file type change should index");

    assert!(
        snapshot
            .deleted_paths
            .contains(&"src/plugin.rs/lib.rs".to_owned())
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == "src/plugin.rs")
    );
}

struct SubmoduleFixture {
    parent: TempGitRepo,
    _child: TempGitRepo,
}

impl SubmoduleFixture {
    fn create(name: &str) -> Self {
        Self::create_inner(name, None)
    }

    fn create_with_submodule_name(name: &str, submodule_name: &str) -> Self {
        Self::create_inner(name, Some(submodule_name))
    }

    fn create_inner(name: &str, submodule_name: Option<&str>) -> Self {
        let child = TempGitRepo::create(&format!("{name}-child"));
        child.write("src/child.rs", "pub fn child_value() -> u32 { 1 }\n");
        child.write(
            "src/unchanged.rs",
            "pub fn unchanged_value() -> u32 { 1 }\n",
        );
        child.git(["add", "."]);
        child.git(["commit", "-m", "child"]);

        let parent = TempGitRepo::create(&format!("{name}-parent"));
        parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
        parent.git(["add", "."]);
        parent.git(["commit", "-m", "parent"]);
        let child_path = child
            .path
            .to_str()
            .expect("child path should be unicode")
            .to_owned();
        if let Some(submodule_name) = submodule_name {
            parent.git([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "--name",
                submodule_name,
                child_path.as_str(),
                "vendor/module",
            ]);
        } else {
            parent.git([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                child_path.as_str(),
                "vendor/module",
            ]);
        }
        parent.git(["commit", "-am", "add submodule"]);

        Self {
            parent,
            _child: child,
        }
    }

    fn parent_registration(&self) -> CodeRepositoryRegistration {
        CodeRepositoryRegistration::new(
            "repo",
            "alias",
            self.parent.path.display().to_string(),
            vec![".".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate")
    }

    fn parent_src_registration(&self) -> CodeRepositoryRegistration {
        CodeRepositoryRegistration::new(
            "repo",
            "alias",
            self.parent.path.display().to_string(),
            vec!["src".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate")
    }

    fn parent_selector(&self) -> CodeRepositorySelector {
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate")
    }

    fn parent_head(&self) -> String {
        self.parent.git_text(["rev-parse", "HEAD"])
    }

    fn previous_hashes(&self) -> Vec<CodeFileFingerprint> {
        build_index_snapshot(
            &self.parent_registration(),
            &self.parent_selector(),
            CodeIndexMode::Full,
            Vec::new(),
        )
        .expect("base full snapshot should build")
        .files
        .into_iter()
        .map(|file| CodeFileFingerprint {
            path: file.path,
            blob_hash: file.blob_hash,
        })
        .collect()
    }

    fn update_submodule_file(&self, content: &str) {
        self.parent.write("vendor/module/src/child.rs", content);
        let submodule_path = self.parent.path.join("vendor/module");
        git_in(
            &submodule_path,
            ["config", "user.email", "relay@example.invalid"],
        );
        git_in(&submodule_path, ["config", "user.name", "Relay Test"]);
        git_in(&submodule_path, ["add", "."]);
        git_in(&submodule_path, ["commit", "-m", "child update"]);
        self.parent.git(["add", "vendor/module"]);
        self.parent.git(["commit", "-m", "update submodule"]);
    }
}

struct TypeChangeFixture {
    parent: TempGitRepo,
    child: TempGitRepo,
}

impl TypeChangeFixture {
    fn regular_file(name: &str) -> Self {
        let child = Self::child_repo(name);
        let parent = TempGitRepo::create(&format!("{name}-parent"));
        parent.write("src/plugin.rs", "pub fn old_plugin() -> u32 { 1 }\n");
        parent.git(["add", "."]);
        parent.git(["commit", "-m", "base file"]);

        Self { parent, child }
    }

    fn submodule(name: &str) -> Self {
        let child = Self::child_repo(name);
        let parent = TempGitRepo::create(&format!("{name}-parent"));
        parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
        parent.git(["add", "."]);
        parent.git(["commit", "-m", "base"]);
        Self::add_submodule(&parent, &child);
        parent.git(["commit", "-am", "base submodule"]);

        Self { parent, child }
    }

    fn child_repo(name: &str) -> TempGitRepo {
        let child = TempGitRepo::create(&format!("{name}-child"));
        child.write("lib.rs", "pub fn child_plugin() -> u32 { 1 }\n");
        child.git(["add", "."]);
        child.git(["commit", "-m", "child"]);
        child
    }

    fn parent_head(&self) -> String {
        self.parent.git_text(["rev-parse", "HEAD"])
    }

    fn previous_hashes(&self) -> Vec<CodeFileFingerprint> {
        build_index_snapshot(
            &self.parent.registration(),
            &self.parent.selector(),
            CodeIndexMode::Full,
            Vec::new(),
        )
        .expect("base full snapshot should build")
        .files
        .into_iter()
        .map(|file| CodeFileFingerprint {
            path: file.path,
            blob_hash: file.blob_hash,
        })
        .collect()
    }

    fn replace_file_with_submodule(&self) {
        self.parent.git(["rm", "src/plugin.rs"]);
        Self::add_submodule(&self.parent, &self.child);
        self.parent
            .git(["commit", "-m", "replace file with submodule"]);
    }

    fn replace_submodule_with_file(&self) {
        self.parent.git(["rm", "-f", "src/plugin.rs"]);
        self.parent
            .write("src/plugin.rs", "pub fn new_plugin() -> u32 { 2 }\n");
        self.parent.git(["add", "."]);
        self.parent
            .git(["commit", "-m", "replace submodule with file"]);
    }

    fn add_submodule(parent: &TempGitRepo, child: &TempGitRepo) {
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
            "src/plugin.rs",
        ]);
    }
}

fn git_in<const N: usize>(path: &std::path::Path, args: [&str; N]) {
    let output = std::process::Command::new("git")
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
