use super::*;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn detects_supported_languages_and_filters_paths() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let trailing_slash_selector = CodeRepositorySelector::new(
        "alias",
        "HEAD",
        vec!["src/".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");

    assert_eq!(language_id("src/lib.rs"), Some("rust"));
    assert!(path_is_selected("src/lib.rs", &registration, &selector));
    assert!(path_is_selected(
        "src/lib.rs",
        &registration,
        &trailing_slash_selector
    ));
    assert!(!path_is_selected("tests/lib.rs", &registration, &selector));
    assert!(!path_is_selected("src/app.py", &registration, &selector));
}

#[test]
fn selector_filters_cannot_widen_registered_scope() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("registration should validate");
    let wider_path_selector =
        CodeRepositorySelector::new("alias", "HEAD", vec!["tests".to_owned()], Vec::new())
            .expect("selector should validate");
    let wider_language_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    assert!(!path_is_selected(
        "tests/lib.rs",
        &registration,
        &wider_path_selector
    ));
    assert!(!path_is_selected(
        "src/app.py",
        &registration,
        &wider_language_selector
    ));
}

#[test]
fn parses_git_name_status_for_rename_copy_and_delete() {
    let changes =
        parse_name_status_z(b"M\0src/lib.rs\0R100\0old.rs\0new.rs\0C100\0a.py\0b.py\0D\0gone.ts\0")
            .expect("name-status should parse");

    assert_eq!(
        changes,
        vec![
            GitChange::AddedOrModified {
                path: "src/lib.rs".to_owned()
            },
            GitChange::Renamed {
                old_path: "old.rs".to_owned(),
                new_path: "new.rs".to_owned()
            },
            GitChange::Copied {
                old_path: "a.py".to_owned(),
                new_path: "b.py".to_owned()
            },
            GitChange::Deleted {
                path: "gone.ts".to_owned()
            }
        ]
    );
}

#[test]
fn worktree_status_uses_destination_path_for_renames_and_copies() {
    let paths = worktree_changed_paths(
        b"R  src/new.rs\0src/old.rs\0C  src/copied.rs\0src/source.rs\0 M src/lib.rs\0",
    );

    assert_eq!(paths[0].path, "src/new.rs");
    assert_eq!(paths[0].deleted_source.as_deref(), Some("src/old.rs"));
    assert_eq!(paths[1].path, "src/copied.rs");
    assert_eq!(paths[1].deleted_source, None);
    assert_eq!(paths[2].path, "src/lib.rs");
    assert_eq!(paths[2].deleted_source, None);
}

#[test]
fn impact_paths_for_copies_only_include_destination() {
    let paths = impact_paths_from_changes(vec![GitChange::Copied {
        old_path: "src/source.rs".to_owned(),
        new_path: "src/copied.rs".to_owned(),
    }]);

    assert_eq!(paths, ["src/copied.rs"]);
}

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
fn worktree_overlay_skips_directories_and_counts_changed_path_skips() {
    let repo = TempGitRepo::create("overlay-skip");
    repo.write("src/lib.rs", "fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
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
    .expect("overlay should skip non-file paths");

    assert_eq!(snapshot.skipped_unchanged_count, 1);
    assert!(snapshot.files.is_empty());
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

#[test]
fn incremental_deletions_are_limited_to_selected_scope() {
    let repo = TempGitRepo::create("incremental-delete-scope");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("docs/out.rs", "fn out_of_scope() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    fs::remove_file(repo.path.join("docs/out.rs")).expect("out-of-scope file should delete");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "delete docs"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::incremental(base, "HEAD").expect("incremental mode should validate"),
        Vec::new(),
    )
    .expect("incremental delete should index");

    assert!(snapshot.deleted_paths.is_empty());
}

#[test]
fn deleted_symbol_names_are_extracted_from_base_diff() {
    let repo = TempGitRepo::create("deleted-symbol-seeds");
    repo.write("src/lib.rs", "fn removed_api() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    fs::remove_file(repo.path.join("src/lib.rs")).expect("source file should delete");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "delete api"]);

    let names =
        deleted_symbol_names_for_diff(&repo.registration(), &repo.selector(), &base, "HEAD")
            .expect("deleted symbols should parse");

    assert_eq!(names, ["removed_api"]);
}

#[test]
fn reference_resolution_prefers_same_path_and_leaves_ambiguous_names_unresolved() {
    let symbols = vec![
        symbol("sym-a", "src/a.rs", "run"),
        symbol("sym-b", "src/b.rs", "run"),
    ];
    let mut references = vec![
        reference("ref-a", "src/a.rs", "run"),
        reference("ref-c", "src/c.rs", "run"),
    ];

    resolve_reference_targets(&symbols, &mut references);

    assert_eq!(
        references[0].target_symbol_snapshot_id.as_deref(),
        Some("sym-a")
    );
    assert_eq!(references[1].target_symbol_snapshot_id, None);
}

fn symbol(id: &str, path: &str, name: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        symbol_snapshot_id: id.to_owned(),
        file_id: format!("file-{id}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: crate::domain::RepositoryCodeRange { start: 0, end: 8 },
        line_range: crate::domain::RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn reference(id: &str, path: &str, name: &str) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        reference_id: id.to_owned(),
        file_id: format!("file-{id}"),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        byte_range: crate::domain::RepositoryCodeRange { start: 0, end: 8 },
        line_range: crate::domain::RepositoryCodeRange { start: 1, end: 1 },
    }
}

struct TempGitRepo {
    path: PathBuf,
}

impl TempGitRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("repo directory should be created");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    fn registration(&self) -> CodeRepositoryRegistration {
        CodeRepositoryRegistration::new(
            "repo",
            "alias",
            self.path.display().to_string(),
            vec!["src".to_owned()],
            vec!["rust".to_owned()],
        )
        .expect("registration should validate")
    }

    fn selector(&self) -> CodeRepositorySelector {
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate")
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for TempGitRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
