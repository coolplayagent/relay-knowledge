use rusqlite::Connection;

use crate::{
    domain::{CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::StorageError,
    storage::sqlite::code::{
        code_tests::{
            retarget_snapshot_scope, retarget_snapshot_to_fact_scope, snapshot_with_chunk,
        },
        initialize_code_schema,
        lifecycle::status::upsert_repository,
    },
};

use super::super::apply_snapshot;
use super::{
    require_fresh_full_snapshot_within_budget, require_incremental_snapshot_within_budget,
};

#[test]
fn direct_framework_facts_budget_their_search_and_fts_writes_before_publication() {
    use crate::domain::{
        CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, FrameworkEdgeKind, FrameworkKind,
        FrameworkNodeKind, RepositoryCodeRange,
    };
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_code_schema(&connection).unwrap();
    register_repository(&mut connection);
    let mut snapshot = snapshot_with_chunk("repo", "src/app.ts", "class App {}");
    let mut baseline_connection = Connection::open_in_memory().unwrap();
    initialize_code_schema(&baseline_connection).unwrap();
    register_repository(&mut baseline_connection);
    let baseline = apply_snapshot(&mut baseline_connection, snapshot.clone()).unwrap();
    let base_budget = CodeIndexResourceBudget::new(1, 1_000_000, 1_000).unwrap();
    let base_rows = super::measure_snapshot_insert_surface(&snapshot, base_budget)
        .unwrap()
        .rows;
    let file = &snapshot.files[0];
    snapshot.framework_nodes.push(CodeFrameworkNodeRecord {
        repository_id: "repo".into(),
        source_scope: snapshot.source_scope.clone(),
        node_id: "component".into(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        framework: FrameworkKind::Angular,
        kind: FrameworkNodeKind::Component,
        name: "App".into(),
        detail: None,
        symbol_snapshot_id: None,
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    });
    snapshot.framework_edges.push(CodeFrameworkEdgeRecord {
        repository_id: "repo".into(),
        source_scope: snapshot.source_scope.clone(),
        edge_id: "render".into(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        framework: FrameworkKind::Angular,
        kind: FrameworkEdgeKind::Renders,
        source_node_id: "component".into(),
        target_node_id: None,
        target_hint: Some("External".into()),
        resolution_state: "unresolved".into(),
        confidence_basis_points: 8000,
        confidence_tier: "structured".into(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    });
    let transaction = connection.transaction().unwrap();
    let too_small = CodeIndexResourceBudget::new(1, 1_000_000, base_rows + 5).unwrap();
    assert!(matches!(
        require_fresh_full_snapshot_within_budget(&transaction, &snapshot, too_small),
        Err(StorageError::CapacityExceeded(_))
    ));
    let exact = CodeIndexResourceBudget::new(1, 1_000_000, base_rows + 6).unwrap();
    assert!(require_fresh_full_snapshot_within_budget(&transaction, &snapshot, exact).is_ok());
    transaction.rollback().unwrap();
    let written: usize = connection.query_row("SELECT (SELECT COUNT(*) FROM code_repository_framework_nodes) + (SELECT COUNT(*) FROM code_repository_framework_edges) + (SELECT COUNT(*) FROM code_repository_search_metadata)", [], |row| row.get(0)).unwrap();
    assert_eq!(written, 0);
    let report = apply_snapshot(&mut connection, snapshot).unwrap();
    assert_eq!(
        report.progress.sqlite_write_count,
        baseline.progress.sqlite_write_count + 2
    );
}

#[test]
fn zero_reference_snapshot_cannot_bypass_the_all_surface_row_budget() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    let transaction = connection.transaction().expect("transaction should begin");
    let snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn bounded() {}");
    assert!(snapshot.references.is_empty());
    let budget =
        CodeIndexResourceBudget::new(1, 1_000_000, 3).expect("test budget should validate");

    let error = require_fresh_full_snapshot_within_budget(&transaction, &snapshot, budget)
        .expect_err("files, symbols, chunks, and search ownership must share one row budget");

    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[test]
fn existing_scope_reservation_requires_durable_staging() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    register_repository(&mut connection);
    let snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn bounded() {}");
    apply_snapshot(&mut connection, snapshot.clone()).expect("initial scope should publish");
    let transaction = connection.transaction().expect("transaction should begin");

    let error = require_fresh_full_snapshot_within_budget(
        &transaction,
        &snapshot,
        CodeIndexResourceBudget::default(),
    )
    .expect_err("an existing scope must never enter direct replacement cleanup");

    assert!(matches!(error, StorageError::DurableStagingRequired(_)));
}

#[test]
fn existing_checkpoint_reservation_requires_durable_staging() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    register_repository(&mut connection);
    connection
        .execute(
            "INSERT INTO code_repository_index_checkpoints (
                 source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                 path_filters_json, language_filters_json, total_path_count,
                 parsed_file_count, committed_file_count, committed_symbol_count,
                 committed_reference_count, committed_chunk_count, batch_count,
                 last_path, resource_budget_json, updated_at_ms, error_message
             ) VALUES (
                 'reserved', 'repo', 'indexing', 'commit', 'tree', '[]', '[]',
                 0, 0, 0, 0, 0, 0, 0, NULL,
                 '{\"max_files_per_batch\":1,\"max_bytes_per_batch\":1,\"max_rows_per_batch\":1}',
                 1, NULL
             )",
            [],
        )
        .expect("checkpoint reservation should insert");
    let mut snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn bounded() {}");
    retarget_snapshot_scope(&mut snapshot, "reserved");
    let transaction = connection.transaction().expect("transaction should begin");

    let error = require_fresh_full_snapshot_within_budget(
        &transaction,
        &snapshot,
        CodeIndexResourceBudget::default(),
    )
    .expect_err("an existing checkpoint must reserve its scope from direct publication");

    assert!(matches!(error, StorageError::DurableStagingRequired(_)));
}

#[test]
fn incremental_base_surface_over_budget_requires_staging_before_target_writes() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    register_repository(&mut connection);
    let mut base = snapshot_with_chunk("repo", "src/lib.rs", "fn bounded() {}");
    base.resolved_commit_sha = "base-commit".to_owned();
    base.tree_hash = "base-tree".to_owned();
    retarget_snapshot_to_fact_scope(&mut base);
    let mut incremental = base.clone();
    apply_snapshot(&mut connection, base).expect("base snapshot should publish");

    incremental.base_resolved_commit_sha = Some("base-commit".to_owned());
    incremental.resolved_commit_sha = "next-commit".to_owned();
    incremental.tree_hash = "next-tree".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 0;
    incremental.skipped_unchanged_count = 1;
    retarget_snapshot_to_fact_scope(&mut incremental);
    let target_scope = incremental.source_scope.clone();
    incremental.files.clear();
    incremental.symbols.clear();
    incremental.references.clear();
    incremental.imports.clear();
    incremental.calls.clear();
    incremental.dependencies.clear();
    incremental.feature_flags.clear();
    incremental.routes.clear();
    incremental.chunks.clear();
    incremental.workspaces.clear();
    incremental.diagnostics.clear();
    incremental.tombstones.clear();

    let transaction = connection.transaction().expect("transaction should begin");
    let error = require_incremental_snapshot_within_budget(
        &transaction,
        &incremental,
        CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("test budget should validate"),
    )
    .expect_err("the cloned base cannot exceed the same writer quantum as the delta");
    assert!(matches!(error, StorageError::DurableStagingRequired(_)));
    transaction
        .rollback()
        .expect("read-only admission transaction should roll back");

    let target_rows = connection
        .query_row(
            "SELECT
                 (SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1)
               + (SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1)
               + (SELECT COUNT(*) FROM code_repository_search_metadata WHERE source_scope = ?1)",
            [target_scope],
            |row| row.get::<_, usize>(0),
        )
        .expect("target row count should load");
    assert_eq!(target_rows, 0);
}

fn register_repository(connection: &mut Connection) {
    upsert_repository(
        connection,
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate"),
    )
    .expect("repository should persist");
}
