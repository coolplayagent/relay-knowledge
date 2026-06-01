use std::{fs, process::Command};

#[cfg(unix)]
use std::os::unix::fs as unix_fs;

use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration,
    CodeRepositorySelector,
};

use super::source::{
    FileSystemScanPolicy, ensure_filesystem_blobs_match_content_hashes,
    filesystem_content_hashes_for_paths, filesystem_tree_hash_for_paths,
    mutate_next_filesystem_policy_read, source_snapshot,
};
use super::{
    SourceGrepKind, SourceGrepRequest, build_index_snapshot, build_index_snapshot_with_base_commit,
    changed_paths_for_diff_with_filters, changed_paths_for_diff_with_path_filters,
    deleted_symbol_names_for_diff, mutate_next_filesystem_full_snapshot_read,
    partition_changed_paths_for_selector, prepare_full_index_plan, preview_repository_scope,
    register_repository, resolve_repository_ref, resolve_repository_ref_with_filters,
    resolve_repository_ref_with_path_filters, resolve_repository_snapshot_with_path_filters,
    source_grep_matches,
    test_fixtures::{TempGitRepo, TempSourceDir},
};

#[test]
fn filtered_non_git_refs_do_not_hash_unselected_broad_directories() {
    let source = TempSourceDir::create("filesystem-src-filter");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write(
        "node_modules/pkg/index.js",
        "export function dependency() {}\n",
    );
    let path_filters = vec!["src".to_owned()];

    let before = resolve_repository_ref_with_path_filters(&source.path, "HEAD", &path_filters)
        .expect("filesystem ref should resolve");
    source.write(
        "node_modules/pkg/index.js",
        "export function changed_dependency() {}\n",
    );
    let after = resolve_repository_ref_with_path_filters(&source.path, "HEAD", &path_filters)
        .expect("filesystem ref should resolve");

    assert_eq!(before, after);
}

#[test]
fn filtered_non_git_refs_do_not_hash_unselected_regular_files() {
    let source = TempSourceDir::create("filesystem-src-regular-filter");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("tests/helper.rs", "pub fn helper() {}\n");
    let path_filters = vec!["src".to_owned()];

    let before = resolve_repository_ref_with_path_filters(&source.path, "HEAD", &path_filters)
        .expect("filesystem ref should resolve");
    source.write("tests/helper.rs", "pub fn changed_helper() {}\n");
    let after = resolve_repository_ref_with_path_filters(&source.path, "HEAD", &path_filters)
        .expect("filesystem ref should resolve");

    assert_eq!(before, after);
}

#[test]
fn filtered_non_git_scan_skips_unrelated_directory_roots() {
    let path_filters = ["src".to_owned()];
    let policy = FileSystemScanPolicy::from_path_filters(path_filters.iter());

    assert!(policy.should_descend_directory("src"));
    assert!(policy.should_descend_directory("src/generated"));
    assert!(policy.should_descend_directory("include"));
    assert!(policy.should_descend_directory("include/public"));
    assert!(policy.should_descend_directory("external_deps"));
    assert!(!policy.should_descend_directory("private"));
    assert!(!policy.should_descend_directory("private/generated"));
}

#[test]
fn filtered_non_git_scan_discovers_include_roots() {
    let source = TempSourceDir::create("filesystem-src-include-discovery");
    source.write(
        "src/main.c",
        "#include \"api.h\"\nint main(void) { return 0; }\n",
    );
    source.write("include/api.h", "int api(void);\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        source.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let (_, batch) = prepare_full_index_plan(
        registration,
        source.selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("filesystem plan should prepare")
    .parse_next_batch()
    .expect("filesystem batch should parse");
    let paths = batch
        .expect("filesystem batch should exist")
        .files
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();

    assert!(paths.contains(&"include/api.h".to_owned()));
}

#[test]
fn default_non_git_scan_skips_unwhitelisted_directory_roots() {
    let path_filters: Vec<String> = Vec::new();
    let policy = FileSystemScanPolicy::from_path_filters(path_filters.iter());

    assert!(policy.should_descend_directory("src"));
    assert!(policy.should_descend_directory("docs/guides"));
    assert!(!policy.should_descend_directory("private"));
    assert!(!policy.should_descend_directory("tmp/cache"));
}

#[test]
fn explicit_root_non_git_filter_includes_broad_directories() {
    let source = TempSourceDir::create("filesystem-root-filter");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("build/generated.rs", "pub fn generated() {}\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        source.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let plan = prepare_full_index_plan(
        registration,
        source.selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("filesystem plan should prepare");

    let (_, batch) = plan
        .parse_next_batch()
        .expect("filesystem batch should parse");
    let paths = batch
        .expect("filesystem batch should exist")
        .files
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();

    assert!(paths.contains(&"build/generated.rs".to_owned()));
}

#[test]
fn non_git_ref_resolution_honors_language_filters() {
    let source = TempSourceDir::create("filesystem-language-filter");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("src/index.ts", "export function indexedTs() {}\n");
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let plan = prepare_full_index_plan(
        source.registration(),
        selector,
        CodeIndexResourceBudget::default(),
    )
    .expect("filesystem plan should prepare");
    let resolved =
        resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &["rust".to_owned()])
            .expect("filesystem ref should resolve with language filters");
    let unfiltered =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");

    assert_eq!(plan.session().resolved_commit_sha, resolved);
    assert_ne!(resolved, unfiltered);
}

#[test]
fn non_git_impact_paths_honor_language_filtered_refs() {
    let source = TempSourceDir::create("filesystem-language-impact");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("src/index.ts", "export function indexedTs() {}\n");
    let language_filters = vec!["rust".to_owned()];
    let base = resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &language_filters)
        .expect("language-filtered filesystem ref should resolve");

    let unchanged =
        changed_paths_for_diff_with_filters(&source.path, &base, "HEAD", &[], &language_filters)
            .expect("language-filtered impact paths should resolve");
    source.write("src/lib.rs", "pub fn indexed_changed() {}\n");
    let changed =
        changed_paths_for_diff_with_filters(&source.path, &base, "HEAD", &[], &language_filters)
            .expect("language-filtered impact paths should resolve");

    assert!(unchanged.is_empty());
    assert_eq!(changed, ["src/lib.rs"]);
}

#[test]
fn non_git_impact_reports_in_scope_edits_between_filtered_filesystem_refs() {
    let source = TempSourceDir::create("filesystem-impact-filtered-ref-edits");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("src/index.ts", "export function indexedTs() {}\n");
    let language_filters = vec!["rust".to_owned()];
    let filtered_base =
        resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &language_filters)
            .expect("filtered filesystem ref should resolve");
    source.write("src/lib.rs", "pub fn changed_rust() {}\n");
    let filtered_head =
        resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &language_filters)
            .expect("filtered filesystem ref should change");

    let paths = changed_paths_for_diff_with_filters(
        &source.path,
        &filtered_base,
        &filtered_head,
        &[],
        &language_filters,
    )
    .expect("filtered filesystem impact refs should resolve");

    assert_ne!(filtered_base, filtered_head);
    assert_eq!(paths, ["src/lib.rs"]);
}

#[test]
fn filesystem_snapshot_hash_ignores_default_excluded_files_unless_explicitly_targeted() {
    let source = TempSourceDir::create("filesystem-excluded-hash");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    source.write("src/events.jsonl", "{\"event\":1}\n");
    let src_filter = ["src".to_owned()];
    let before = source_snapshot(
        &source.path,
        "HEAD",
        FileSystemScanPolicy::from_path_filters(src_filter.iter()),
    )
    .expect("filesystem snapshot should build")
    .resolved_commit_sha;
    source.write("src/events.jsonl", "{\"event\":2}\n");
    let after = source_snapshot(
        &source.path,
        "HEAD",
        FileSystemScanPolicy::from_path_filters(src_filter.iter()),
    )
    .expect("filesystem snapshot should build")
    .resolved_commit_sha;
    let explicit_filter = ["src/events.jsonl".to_owned()];
    let explicit_before = source_snapshot(
        &source.path,
        "HEAD",
        FileSystemScanPolicy::from_path_filters(explicit_filter.iter()),
    )
    .expect("filesystem snapshot should build")
    .resolved_commit_sha;
    source.write("src/events.jsonl", "{\"event\":3}\n");
    let explicit_after = source_snapshot(
        &source.path,
        "HEAD",
        FileSystemScanPolicy::from_path_filters(explicit_filter.iter()),
    )
    .expect("filesystem snapshot should build")
    .resolved_commit_sha;

    assert_eq!(before, after);
    assert_ne!(explicit_before, explicit_after);
}

#[test]
fn filesystem_planned_blob_check_rejects_changed_delta_bytes() {
    let source = TempSourceDir::create("filesystem-delta-stale-bytes");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let paths = vec!["src/lib.rs".to_owned()];
    let planned_hashes = filesystem_content_hashes_for_paths(&source.path, &paths)
        .expect("planned hashes should build");
    let changed_blobs = vec![b"pub fn changed() {}\n".to_vec()];

    let error = ensure_filesystem_blobs_match_content_hashes(
        "filesystem:planned",
        &paths,
        &changed_blobs,
        &planned_hashes,
    )
    .expect_err("changed planned bytes should be rejected");

    assert!(
        error
            .to_string()
            .contains("no longer matches planned filesystem file src/lib.rs")
    );
}

#[test]
fn filesystem_full_snapshot_rejects_changed_bytes_after_hash_plan() {
    let source = TempSourceDir::create("filesystem-full-stale-bytes");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let root = source
        .path
        .canonicalize()
        .expect("source root should canonicalize");
    mutate_next_filesystem_full_snapshot_read(
        root,
        "src/lib.rs",
        b"pub fn changed_after_plan() {}\n",
    );

    let error = build_index_snapshot(
        &source.registration(),
        &source.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect_err("changed full snapshot bytes should be rejected");

    assert!(
        error
            .to_string()
            .contains("no longer matches planned filesystem file src/lib.rs")
    );
}

#[test]
fn filtered_non_git_ref_matches_explicit_broad_directory_index() {
    let source = TempSourceDir::create("filesystem-build-filter");
    source.write("src/lib.rs", "pub fn ignored() {}\n");
    source.write("build/generated.rs", "pub fn generated() {}\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        source.path.display().to_string(),
        vec!["build".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let plan = prepare_full_index_plan(
        registration.clone(),
        selector,
        CodeIndexResourceBudget::default(),
    )
    .expect("filesystem plan should prepare");
    let resolved =
        resolve_repository_ref_with_path_filters(&source.path, "HEAD", &registration.path_filters)
            .expect("filesystem ref should resolve");

    assert_eq!(plan.session().resolved_commit_sha, resolved);
}

#[test]
fn queued_non_git_full_index_rejects_changed_synthetic_ref() {
    let source = TempSourceDir::create("filesystem-queued-ref");
    source.write("src/lib.rs", "pub fn queued() {}\n");
    let registration = source.registration();
    let selector = source.selector();
    let plan = prepare_full_index_plan(
        registration.clone(),
        selector,
        CodeIndexResourceBudget::default(),
    )
    .expect("filesystem plan should prepare");
    let queued_commit = plan.session().resolved_commit_sha;
    source.write("src/lib.rs", "pub fn changed_queued() {}\n");
    let replay_selector =
        CodeRepositorySelector::new("alias", queued_commit, Vec::new(), Vec::new())
            .expect("selector should validate");

    let error = prepare_full_index_plan(
        registration,
        replay_selector,
        CodeIndexResourceBudget::default(),
    )
    .expect_err("stale synthetic filesystem ref should be rejected");

    assert!(
        error
            .to_string()
            .contains("no longer matches live indexed scope")
    );
}

#[test]
fn non_git_batch_reads_reject_changed_planned_files() {
    let source = TempSourceDir::create("filesystem-batch-stale");
    source.write("src/a.rs", "pub fn a() {}\n");
    source.write("src/b.rs", "pub fn b() {}\n");
    let budget =
        CodeIndexResourceBudget::new(1, 1024 * 1024, 10_000).expect("budget should validate");
    let plan = prepare_full_index_plan(source.registration(), source.selector(), budget)
        .expect("filesystem plan should prepare");
    let (plan, first_batch) = plan.parse_next_batch().expect("first batch should parse");
    let first_paths = first_batch
        .expect("first batch should exist")
        .files
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();
    source.write("src/b.rs", "pub fn changed_b() {}\n");

    let error = plan
        .parse_next_batch()
        .expect_err("changed planned file should reject later batch read");

    assert_eq!(first_paths, ["src/a.rs"]);
    assert!(
        error
            .to_string()
            .contains("no longer matches planned filesystem file src/b.rs")
    );
}

#[test]
fn explicit_stored_filesystem_commit_resolves_without_live_verification() {
    let source = TempSourceDir::create("filesystem-stored-ref-resolution");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    source.write("src/lib.rs", "pub fn changed_after_index() {}\n");

    let resolved = resolve_repository_ref_with_path_filters(&source.path, &commit, &[])
        .expect("stored filesystem ref should resolve from storage identity");
    let (snapshot_commit, tree_hash) =
        resolve_repository_snapshot_with_path_filters(&source.path, &commit, &[])
            .expect("stored filesystem snapshot should resolve from storage identity");

    assert_eq!(resolved, commit);
    assert_eq!(snapshot_commit, commit);
    assert_eq!(tree_hash, commit);
}

#[test]
fn stored_filesystem_commit_resolves_after_git_metadata_appears() {
    let source = TempSourceDir::create("filesystem-stored-ref-after-git");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let resolved = resolve_repository_ref(&source.path, &commit)
        .expect("stored filesystem ref should resolve");
    let (snapshot_commit, tree_hash) =
        resolve_repository_snapshot_with_path_filters(&source.path, &commit, &[])
            .expect("stored filesystem snapshot should resolve");

    assert_eq!(resolved, commit);
    assert_eq!(snapshot_commit, commit);
    assert_eq!(tree_hash, commit);
}

#[test]
fn stored_filesystem_commit_preview_survives_later_git_metadata() {
    let source = TempSourceDir::create("filesystem-preview-after-git");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let registration = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect("filesystem source should register");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selector = CodeRepositorySelector::new("alias", &commit, Vec::new(), Vec::new())
        .expect("selector should validate");

    let preview = preview_repository_scope(&registration, &selector)
        .expect("stored filesystem preview should not use git");

    assert_eq!(preview.resolved_commit_sha, commit);
    assert_eq!(preview.selected_file_count, 1);
}

#[test]
fn stored_filesystem_commit_impact_survives_later_git_metadata() {
    let source = TempSourceDir::create("filesystem-impact-after-git");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let paths = changed_paths_for_diff_with_filters(&source.path, &commit, &commit, &[], &[])
        .expect("stored filesystem impact refs should not use git diff");

    assert!(paths.is_empty());
}

#[test]
fn stored_filesystem_commit_impact_partition_survives_later_git_metadata() {
    let source = TempSourceDir::create("filesystem-impact-partition-after-git");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let registration = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect("filesystem source should register");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selector = CodeRepositorySelector::new("alias", &commit, Vec::new(), Vec::new())
        .expect("selector should validate");

    let groups = partition_changed_paths_for_selector(
        &registration,
        &selector,
        vec![
            "src/lib.rs".to_owned(),
            "node_modules/pkg/index.js".to_owned(),
        ],
    )
    .expect("stored filesystem impact partition should not use git");

    assert_eq!(groups.in_scope_changed_paths, ["src/lib.rs"]);
    assert_eq!(
        groups.out_of_scope_changed_paths,
        ["node_modules/pkg/index.js"]
    );
}

#[test]
fn stored_filesystem_commit_deleted_symbols_skip_git_after_metadata_appears() {
    let source = TempSourceDir::create("filesystem-deleted-symbols-after-git");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let registration = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect("filesystem source should register");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selector = CodeRepositorySelector::new("alias", &commit, Vec::new(), Vec::new())
        .expect("selector should validate");

    let names = deleted_symbol_names_for_diff(&registration, &selector, &commit, &commit)
        .expect("filesystem refs should not reach git deleted-symbol diff");

    assert!(names.is_empty());
}

#[test]
fn filesystem_incremental_uses_stored_base_after_git_metadata_appears() {
    let source = TempSourceDir::create("filesystem-incremental-after-git");
    source.write("src/lib.rs", "pub fn indexed() -> u32 { 1 }\n");
    let registration = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect("filesystem source should register");
    let selector = source.selector();
    let base_snapshot = build_index_snapshot_with_base_commit(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
        None,
    )
    .expect("base filesystem index should build");
    let previous_hashes = base_snapshot
        .files
        .iter()
        .map(|file| CodeFileFingerprint {
            path: file.path.clone(),
            blob_hash: file.blob_hash.clone(),
        })
        .collect::<Vec<_>>();
    let base_commit = base_snapshot.resolved_commit_sha.clone();
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    source.write("src/lib.rs", "pub fn indexed() -> u32 { 2 }\n");
    source.write("src/new.rs", "pub fn added_after_git_metadata() {}\n");

    let snapshot = build_index_snapshot_with_base_commit(
        &registration,
        &selector,
        CodeIndexMode::incremental(base_commit.clone(), "HEAD")
            .expect("incremental mode should validate"),
        previous_hashes.clone(),
        Some(base_commit.clone()),
    )
    .expect("stored filesystem base should keep incremental update on filesystem delta");

    assert!(snapshot.resolved_commit_sha.starts_with("filesystem:"));
    assert_eq!(
        snapshot.base_resolved_commit_sha.as_deref(),
        Some(base_commit.as_str())
    );
    assert!(snapshot.files.iter().any(|file| file.path == "src/lib.rs"));
    assert!(snapshot.files.iter().any(|file| file.path == "src/new.rs"));

    let overlay_snapshot = build_index_snapshot_with_base_commit(
        &registration,
        &selector,
        CodeIndexMode::WorktreeOverlay,
        previous_hashes,
        Some(base_commit.clone()),
    )
    .expect("stored filesystem base should keep worktree overlay on filesystem delta");

    assert!(
        overlay_snapshot
            .resolved_commit_sha
            .starts_with("filesystem:")
    );
    assert_eq!(
        overlay_snapshot.base_resolved_commit_sha.as_deref(),
        Some(base_commit.as_str())
    );
}

#[test]
fn non_git_impact_paths_are_empty_when_scoped_refs_match() {
    let source = TempSourceDir::create("filesystem-impact-unchanged");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let base = resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");

    let paths = changed_paths_for_diff_with_path_filters(&source.path, &base, "HEAD", &[])
        .expect("filesystem impact paths should resolve");

    assert!(paths.is_empty());
}

#[test]
fn non_git_impact_paths_use_indexed_broad_scope() {
    let source = TempSourceDir::create("filesystem-impact-build");
    source.write("src/lib.rs", "pub fn ignored() {}\n");
    source.write("build/generated.rs", "pub fn generated() {}\n");
    let path_filters = vec!["build".to_owned()];

    let paths =
        changed_paths_for_diff_with_path_filters(&source.path, "previous", "HEAD", &path_filters)
            .expect("filesystem impact paths should resolve");

    assert_eq!(paths, ["build/generated.rs"]);
}

#[cfg(unix)]
#[test]
fn filesystem_content_hashes_reject_symlink_swaps() {
    let source = TempSourceDir::create("filesystem-symlink-swap");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let outside = source.path.with_file_name(format!(
        "{}-outside.rs",
        source
            .path
            .file_name()
            .expect("source path should have a file name")
            .to_string_lossy()
    ));
    fs::write(&outside, "pub fn outside_scope() {}\n").expect("outside file should be written");
    fs::remove_file(source.path.join("src/lib.rs")).expect("indexed file should be replaceable");
    unix_fs::symlink(&outside, source.path.join("src/lib.rs"))
        .expect("fixture symlink should be created");
    let paths = vec!["src/lib.rs".to_owned()];

    let error = filesystem_content_hashes_for_paths(&source.path, &paths)
        .expect_err("symlink replacement should be rejected");
    let _ = fs::remove_file(outside);

    assert!(error.to_string().contains("is a symlink"));
}

#[cfg(unix)]
#[test]
fn filesystem_content_hashes_reject_symlink_ancestor_swaps() {
    let source = TempSourceDir::create("filesystem-symlink-ancestor-swap");
    source.write("src/lib.rs", "pub fn indexed() {}\n");
    let outside = source.path.with_file_name(format!(
        "{}-outside",
        source
            .path
            .file_name()
            .expect("source path should have a file name")
            .to_string_lossy()
    ));
    fs::create_dir_all(&outside).expect("outside directory should be created");
    fs::write(outside.join("lib.rs"), "pub fn outside_scope() {}\n")
        .expect("outside file should be written");
    fs::remove_file(source.path.join("src/lib.rs")).expect("indexed file should be removable");
    fs::remove_dir(source.path.join("src")).expect("indexed directory should be replaceable");
    unix_fs::symlink(&outside, source.path.join("src"))
        .expect("fixture symlink directory should be created");
    let paths = vec!["src/lib.rs".to_owned()];

    let error = filesystem_content_hashes_for_paths(&source.path, &paths)
        .expect_err("symlink ancestor replacement should be rejected");
    let _ = fs::remove_dir_all(outside);

    assert!(error.to_string().contains("component src is a symlink"));
}

#[test]
fn non_git_incremental_deletes_removed_discovered_root_file() {
    let source = TempSourceDir::create("filesystem-discovered-delete");
    source.write("src/lib.rs", "pub fn local_entry() {}\n");
    source.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn external_entry() {}\n",
    );
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        source.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = source.selector();
    let base_snapshot = build_index_snapshot_with_base_commit(
        &registration,
        &selector,
        CodeIndexMode::Full,
        Vec::new(),
        None,
    )
    .expect("base filesystem index should build");
    let previous_hashes = base_snapshot
        .files
        .iter()
        .map(|file| CodeFileFingerprint {
            path: file.path.clone(),
            blob_hash: file.blob_hash.clone(),
        })
        .collect::<Vec<_>>();
    let base_commit = base_snapshot.resolved_commit_sha.clone();
    fs::remove_file(source.path.join("external_deps/rust_sdk/lib.rs"))
        .expect("discovered source file should delete");
    let snapshot = build_index_snapshot_with_base_commit(
        &registration,
        &selector,
        CodeIndexMode::incremental(base_commit.clone(), "HEAD")
            .expect("incremental mode should validate"),
        previous_hashes,
        Some(base_commit),
    )
    .expect("incremental filesystem index should build");

    assert!(
        snapshot
            .path_filters
            .contains(&"external_deps/rust_sdk".to_owned())
    );
    assert_eq!(
        snapshot.deleted_paths,
        ["external_deps/rust_sdk/lib.rs".to_owned()]
    );
}

#[test]
fn corrupt_git_metadata_is_not_registered_as_filesystem_source() {
    let source = TempSourceDir::create("filesystem-corrupt-git");
    source.write(".git", "gitdir: /definitely/missing/relay-knowledge\n");

    let error = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect_err("corrupt git metadata should not fall back to filesystem source");

    assert!(error.to_string().contains("not a git repository"));
}

#[test]
fn git_registration_rejects_synthetic_filesystem_ref() {
    let repo = TempGitRepo::create("git-registration-filesystem-ref");
    repo.write("src/lib.rs", "pub fn tracked() {}\n");
    repo.write("src/untracked.rs", "pub fn untracked() {}\n");
    repo.git(["add", "src/lib.rs"]);
    repo.git(["commit", "-m", "initial"]);
    let paths = vec!["src/lib.rs".to_owned(), "src/untracked.rs".to_owned()];
    let filesystem_ref = filesystem_tree_hash_for_paths(&repo.path, &paths)
        .expect("synthetic filesystem ref should build");
    let selector = CodeRepositorySelector::new("alias", filesystem_ref, Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = prepare_full_index_plan(
        repo.registration(),
        selector,
        CodeIndexResourceBudget::default(),
    )
    .expect_err("git registration should not accept filesystem authority");

    assert!(error.to_string().contains("filesystem:"));
}

#[test]
fn broad_filesystem_commit_source_fallback_accepts_narrow_request_scope() {
    let source = TempSourceDir::create("filesystem-broad-fallback-narrow-query");
    source.write("src/lib.rs", "pub fn fallback_target() {}\n");
    source.write("src/other.rs", "pub fn other_target() {}\n");
    let registration = source.registration();
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");

    let outcome = source_grep_matches(
        &registration,
        &commit,
        SourceGrepRequest {
            query: "fallback_target".to_owned(),
            paths: vec!["src/lib.rs".to_owned()],
            path_filters: vec!["src/lib.rs".to_owned()],
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source fallback should verify the stored broad scope");

    assert!(outcome.degraded_reason.is_none());
    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/lib.rs");
}

#[test]
fn stored_filesystem_commit_source_fallback_refuses_live_changes() {
    let source = TempSourceDir::create("filesystem-stored-commit");
    source.write("src/lib.rs", "pub fn fallback_target() {}\n");
    let registration = source.registration();
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    source.write("src/lib.rs", "pub fn changed_target() {}\n");

    let outcome = source_grep_matches(
        &registration,
        &commit,
        SourceGrepRequest {
            query: "changed_target".to_owned(),
            paths: vec!["src/lib.rs".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source fallback should handle stale filesystem source");

    assert!(outcome.matches.is_empty());
    assert!(
        outcome
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no longer matches live"))
    );
}

#[test]
fn stored_filesystem_commit_source_fallback_rechecks_bytes_after_read() {
    let source = TempSourceDir::create("filesystem-fallback-race");
    source.write("src/lib.rs", "pub fn fallback_target() {}\n");
    let registration = source.registration();
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    mutate_next_filesystem_policy_read(
        source.path.clone(),
        "src/lib.rs",
        b"pub fn changed_during_read() {}\n",
    );

    let outcome = source_grep_matches(
        &registration,
        &commit,
        SourceGrepRequest {
            query: "changed_during_read".to_owned(),
            paths: vec!["src/lib.rs".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source fallback should handle filesystem read races");

    assert!(outcome.matches.is_empty());
    assert!(
        outcome
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no longer matches"))
    );
}

#[test]
fn language_filtered_filesystem_commit_source_fallback_reads_current_scope() {
    let source = TempSourceDir::create("filesystem-language-fallback");
    source.write("src/lib.rs", "pub fn fallback_rust_target() {}\n");
    source.write(
        "src/index.ts",
        "export function fallbackTypeScriptTarget() {}\n",
    );
    let language_filters = vec!["rust".to_owned()];
    let registration = source.registration();
    let commit = resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &language_filters)
        .expect("language-filtered filesystem ref should resolve");

    let outcome = source_grep_matches(
        &registration,
        &commit,
        SourceGrepRequest {
            query: "fallback_rust_target".to_owned(),
            paths: vec!["src/lib.rs".to_owned()],
            path_filters: Vec::new(),
            language_filters,
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source fallback should verify language-filtered filesystem source");

    assert!(outcome.degraded_reason.is_none());
    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/lib.rs");
}

#[test]
fn stored_filesystem_commit_source_fallback_survives_later_git_metadata() {
    let source = TempSourceDir::create("filesystem-fallback-after-git");
    source.write("src/lib.rs", "pub fn fallback_after_git_target() {}\n");
    let registration = register_repository(&source.path, "alias", Vec::new(), Vec::new())
        .expect("filesystem source should register");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");
    let output = Command::new("git")
        .arg("init")
        .current_dir(&source.path)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let outcome = source_grep_matches(
        &registration,
        &commit,
        SourceGrepRequest {
            query: "fallback_after_git_target".to_owned(),
            paths: vec!["src/lib.rs".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::Definition,
        },
    )
    .expect("source fallback should verify stored filesystem ref");

    assert!(outcome.degraded_reason.is_none());
    assert_eq!(outcome.matches.len(), 1);
    assert_eq!(outcome.matches[0].path, "src/lib.rs");
}
