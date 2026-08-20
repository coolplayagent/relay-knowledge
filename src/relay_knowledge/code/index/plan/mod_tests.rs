// Direct tests for bounded index planning.

use super::*;

#[test]
fn parser_worker_count_keeps_tiny_batches_serial() {
    assert_eq!(worker_count(7, 32 * 1024), 1);
}

#[test]
fn parser_worker_count_scales_with_bounded_batch_work() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(96, 4 * 1024 * 1024);

    assert_eq!(workers, available.min(8).min(96));
    assert!(workers >= 1);
}

#[test]
fn parser_worker_count_caps_thread_fanout_for_small_byte_batches() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(40, 128 * 1024);

    assert_eq!(workers, available.min(3).min(40));
}

#[test]
fn batch_row_count_includes_feature_flags() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    build.feature_flags = crate::code::feature_flags::extract_feature_flags(
        crate::code::feature_flags::FeatureFlagFileInput {
            repository_id: &build.repository_id,
            source_scope: &build.source_scope,
            file_id: "file",
            path: "src/lib.rs",
            language_id: "rust",
            content: "if env::var(\"CHECKOUT_V2\").is_ok() && env::var(\"PAYMENTS_V2\").is_ok() {}",
            config_facts: &[],
        },
    )
    .expect("feature flags should extract");

    assert_eq!(batch_row_count(&build), 2);
}

#[test]
fn incremental_batch_partition_respects_all_resource_budgets() {
    let files = vec![
        indexed_file("a.rs", 60),
        indexed_file("b.rs", 60),
        indexed_file("c.rs", 1),
    ];
    let rows = BTreeMap::from([
        ("a.rs".to_owned(), 4),
        ("b.rs".to_owned(), 4),
        ("c.rs".to_owned(), 1),
    ]);

    assert_eq!(
        next_incremental_batch_end(
            &files,
            &rows,
            0,
            CodeIndexResourceBudget::new(3, 100, 10).expect("budget"),
        ),
        1
    );
    assert_eq!(
        next_incremental_batch_end(
            &files,
            &rows,
            0,
            CodeIndexResourceBudget::new(3, 1_000, 5).expect("budget"),
        ),
        1
    );
    assert_eq!(
        next_incremental_batch_end(
            &files,
            &rows,
            0,
            CodeIndexResourceBudget::new(1, 1_000, 100).expect("budget"),
        ),
        1
    );
}

#[test]
fn incremental_batch_partition_admits_one_oversized_file() {
    let files = vec![indexed_file("large.rs", 2_000)];
    let rows = BTreeMap::from([("large.rs".to_owned(), 200)]);

    assert_eq!(
        next_incremental_batch_end(
            &files,
            &rows,
            0,
            CodeIndexResourceBudget::new(1, 100, 10).expect("budget"),
        ),
        1
    );
}

#[test]
fn incremental_session_counts_deletion_only_changed_paths() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let snapshot = CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: "target-scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
        resolved_commit_sha: "target".to_owned(),
        tree_hash: "target-tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: 1,
        skipped_unchanged_count: 3,
        deleted_paths: vec!["src/removed.rs".to_owned()],
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    };
    let plan = CodeIndexPlan {
        registration,
        root: PathBuf::from("/tmp/repo"),
        commit: "target".to_owned(),
        tree_hash: "target-tree".to_owned(),
        source_scope: "target-scope".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        source_kind: RepositorySourceKind::Git,
        filesystem_path_hashes: BTreeMap::new(),
        paths: Vec::new(),
        workspaces: Vec::new(),
        cursor: 0,
        next_batch_index: 1,
        resource_budget: CodeIndexResourceBudget::default(),
        incremental: Some(IncrementalPlanData {
            rows_by_path: BTreeMap::new(),
            snapshot,
            cursor: 0,
        }),
    };

    let session = plan.session();

    assert_eq!(session.total_path_count, 1);
    assert_eq!(session.changed_path_count, 1);
    assert_eq!(session.deleted_paths, vec!["src/removed.rs"]);
}

fn indexed_file(path: &str, byte_len: usize) -> crate::domain::RepositoryCodeFileRecord {
    crate::domain::RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        file_id: format!("file-{path}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("hash-{path}"),
        byte_len,
        line_count: 1,
        parse_status: crate::domain::CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}
