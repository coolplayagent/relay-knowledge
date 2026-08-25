use std::collections::BTreeSet;

use rusqlite::Connection;

use super::{CloneIdentity, all_clone_tables, base, page_control_bytes, progress, table_count};
use crate::{domain::CodeIndexResourceBudget, storage::StorageError};

#[test]
fn durable_clone_plan_excludes_the_singleton_manifest() {
    let tables = all_clone_tables()
        .map(|table| table.table)
        .collect::<Vec<_>>();

    assert_eq!(tables.len(), table_count());
    assert!(tables.contains(&"code_repository_reference_search_groups"));
    assert!(!tables.contains(&"code_repository_reference_search_manifests"));
}

#[test]
fn dense_single_file_uses_the_persisted_actual_fact_proof() {
    let connection = proof_database(40_001, 8, 1_000_000);

    let proof = base::step_proof(&connection, "base").expect("exact proof should load");

    assert_eq!(proof.source_fact_row_upper_bound, 40_001);
    assert_eq!(
        proof.max_steps,
        40_001usize
            .checked_mul(5)
            .and_then(|value| value.checked_add(table_count() + 4))
            .expect("fixture proof should fit")
    );
}

#[test]
fn legacy_completed_checkpoint_without_an_actual_fact_proof_fails_closed() {
    let connection = proof_database(0, 150_000, 16 * 1024 * 1024);

    let error = base::step_proof(&connection, "base")
        .expect_err("an upgraded legacy scope must not infer facts from its batch budget");

    assert!(
        matches!(error, StorageError::CapacityExceeded(message) if message.contains("predates"))
    );
}

#[test]
fn scope_without_a_checkpoint_uses_the_same_typed_staging_fallback() {
    let connection = proof_database(1, 1, 1_000_000);

    let error = base::step_proof(&connection, "missing")
        .expect_err("a scope without an actual fact proof must not enter clone initialization");

    assert!(
        matches!(error, StorageError::CapacityExceeded(message) if message.contains("no durable fact-row proof"))
    );
}

#[test]
fn page_control_counts_long_scope_and_repository_in_both_durable_rows() {
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 16).expect("budget should validate");
    let short_progress = sample_progress("s", "r", budget);
    let short_identity = sample_identity("s", "r", budget);
    let long_scope = "s".repeat(257);
    let long_repository = "r".repeat(193);
    let long_progress = sample_progress(&long_scope, &long_repository, budget);
    let long_identity = sample_identity(&long_scope, &long_repository, budget);

    let short = page_control_bytes(&short_progress, &short_identity)
        .expect("short control surface should measure");
    let long = page_control_bytes(&long_progress, &long_identity)
        .expect("long control surface should measure");

    assert_eq!(
        long - short,
        2 * ((long_scope.len() - 1) + (long_repository.len() - 1))
    );
}

#[test]
fn terminal_cleanup_surface_counts_every_frozen_path_and_progress_owner() {
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 16).expect("budget should validate");
    let progress = sample_progress("target", "repo", budget);
    let paths = ["a.rs".to_owned(), "nested/b.rs".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();

    let (rows, bytes) =
        progress::cleanup_surface(&progress, &paths).expect("cleanup should measure");
    let path_bytes = paths
        .iter()
        .map(|path| {
            super::admission::ROW_STORAGE_OVERHEAD_BYTES + progress.source_scope.len() + path.len()
        })
        .sum::<usize>();

    assert_eq!(rows, paths.len() + 1);
    assert!(
        bytes > path_bytes,
        "progress deletion must also be reserved"
    );
}

fn proof_database(
    committed_fact_row_count: usize,
    max_rows_per_batch: usize,
    max_bytes_per_batch: usize,
) -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_index_checkpoints (
                 source_scope TEXT PRIMARY KEY,
                 state TEXT NOT NULL,
                 committed_fact_row_count INTEGER NOT NULL,
                 resource_budget_json TEXT NOT NULL
             );",
        )
        .expect("proof schema should initialize");
    let budget = CodeIndexResourceBudget::new(1, max_bytes_per_batch, max_rows_per_batch)
        .expect("budget should validate");
    connection
        .execute(
            "INSERT INTO code_repository_index_checkpoints VALUES (?1, 'completed', ?2, ?3)",
            rusqlite::params![
                "base",
                committed_fact_row_count,
                serde_json::to_string(&budget).expect("budget should serialize")
            ],
        )
        .expect("proof should insert");
    connection
}

fn sample_progress(
    source_scope: &str,
    repository_id: &str,
    budget: CodeIndexResourceBudget,
) -> progress::CloneProgress {
    progress::CloneProgress {
        source_scope: source_scope.to_owned(),
        repository_id: repository_id.to_owned(),
        base_scope: "base".to_owned(),
        task_id: "task".to_owned(),
        delta_digest: "digest".to_owned(),
        phase: progress::PHASE_TABLES.to_owned(),
        table_ordinal: 0,
        completed_page_ordinal: 0,
        cursor_key: None,
        cursor_tiebreaker: None,
        completed_table_ordinal: None,
        expected_table_rows: None,
        scanned_table_rows: 0,
        copied_table_rows: 0,
        scanned_total_rows: 0,
        copied_total_rows: 0,
        copied_total_bytes: 0,
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        cloned_diagnostic_count: 0,
        cloned_reference_group_count: 0,
        cloned_search_document_count: 0,
        base_manifest_reference_count: 0,
        base_manifest_group_count: 0,
        scanned_reference_occurrence_count: 0,
        scanned_reference_row_count: 0,
        scanned_reference_group_count: 0,
        scanned_reference_search_owner_count: 0,
        base_source_fact_row_upper_bound: 1,
        page_row_limit: budget.max_rows_per_batch,
        page_byte_limit: budget.max_bytes_per_batch,
    }
}

fn sample_identity(
    source_scope: &str,
    repository_id: &str,
    resource_budget: CodeIndexResourceBudget,
) -> CloneIdentity {
    CloneIdentity {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: "base-commit".to_owned(),
        resolved_commit_sha: "next-commit".to_owned(),
        tree_hash: "next-tree".to_owned(),
        path_filters_json: "[]".to_owned(),
        language_filters_json: "[]".to_owned(),
        delta_digest: "digest".to_owned(),
        affected_paths: BTreeSet::new(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        resource_budget,
    }
}
