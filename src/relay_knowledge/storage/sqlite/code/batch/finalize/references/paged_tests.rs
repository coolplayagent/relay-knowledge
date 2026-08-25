use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use super::{
    normalize_unresolved,
    paged::{ReferenceResolutionAdvance, advance, initialize, page_control_bytes},
    paged_sql, resolve,
};
use crate::{
    domain::{
        CodeIndexResourceBudget, CodeReferenceResolution, CodeReferenceResolutionStage,
        code_reference_resolution_cursor_digest,
    },
    storage::StorageError,
};

pub(super) static REFERENCE_SQL_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub(super) static REFERENCE_SQL_TRACE_TEST: Mutex<()> = Mutex::new(());

#[test]
fn reference_resolution_pages_obey_row_cap_and_match_coarse_semantics() {
    let mut paged = reference_database();
    let mut coarse = reference_database();
    seed_semantic_fixture(&mut paged);
    seed_semantic_fixture(&mut coarse);

    let resource_budget = budget(1_048_576, 4);
    initialize_and_commit(&mut paged, resource_budget, 5);
    assert_eq!(
        advance_and_commit(&mut paged, cursor(0, 0, None), resource_budget, 5),
        pending(1, 2, Some("reference:02"))
    );
    assert_progress(&paged, 1, Some("reference:02"), 2, 2);
    assert_eq!(
        advance_and_commit(
            &mut paged,
            cursor(1, 2, Some("reference:02")),
            resource_budget,
            5,
        ),
        pending(2, 4, Some("reference:04"))
    );
    assert_progress(&paged, 2, Some("reference:04"), 4, 2);
    assert_eq!(
        advance_and_commit(
            &mut paged,
            cursor(2, 4, Some("reference:04")),
            resource_budget,
            5,
        ),
        pending(3, 5, Some("reference:05"))
    );
    assert_progress(&paged, 3, Some("reference:05"), 5, 1);
    assert_eq!(
        advance_and_commit(
            &mut paged,
            cursor(3, 5, Some("reference:05")),
            resource_budget,
            5,
        ),
        ReferenceResolutionAdvance::Complete
    );

    let transaction = coarse
        .transaction()
        .expect("coarse transaction should open");
    normalize_unresolved(&transaction, "scope").expect("coarse normalization should succeed");
    resolve(&transaction, "scope").expect("coarse resolution should succeed");
    transaction
        .commit()
        .expect("coarse transaction should commit");
    assert_eq!(resolution_rows(&paged), resolution_rows(&coarse));
}

#[test]
fn reference_resolution_page_enforces_exact_byte_boundary() {
    let mut admitted = reference_database();
    seed_single_reference(&mut admitted);
    let first_row_bytes = first_row_bytes(&mut admitted);
    let admitted_budget = budget(first_row_bytes, 3);
    initialize_and_commit(&mut admitted, admitted_budget, 1);
    assert_eq!(
        advance_and_commit(&mut admitted, cursor(0, 0, None), admitted_budget, 1),
        pending(1, 1, Some("reference:01"))
    );

    let mut rejected = reference_database();
    seed_single_reference(&mut rejected);
    initialize_and_commit(&mut rejected, budget(first_row_bytes - 1, 3), 1);
    let transaction = rejected
        .transaction()
        .expect("rejected transaction should open");
    let error = advance(
        &transaction,
        "scope",
        cursor(0, 0, None),
        budget(first_row_bytes - 1, 3),
        1,
    )
    .expect_err("one byte below the exact row size must reject the page");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    assert_progress(&rejected, 0, None, 0, 0);
}

#[test]
fn reference_resolution_page_charges_long_checkpoint_control_bytes_before_owner_write() {
    let mut connection = reference_database();
    seed_single_reference(&mut connection);
    connection
        .execute(
            "UPDATE code_repository_index_checkpoints SET path_filters_json = ?1
             WHERE source_scope = 'scope'",
            ["x".repeat(8 * 1024)],
        )
        .expect("long but valid checkpoint filter should seed");
    let page_bytes = first_row_bytes(&mut connection);
    let resource_budget = budget(page_bytes - 1, 3);
    initialize_and_commit(&mut connection, resource_budget, 1);
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(
        &transaction,
        "scope",
        cursor(0, 0, None),
        resource_budget,
        1,
    )
    .expect_err("owner plus exact full checkpoint/progress records must fit the byte quantum");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
    transaction
        .rollback()
        .expect("rejected long-control page should roll back");
    assert_eq!(stale_reference_count(&connection), 1);
    assert_progress(&connection, 0, None, 0, 0);
}

#[test]
fn reference_resolution_page_reserves_progress_and_checkpoint_mutations() {
    let mut connection = reference_database();
    seed_single_reference(&mut connection);
    let transaction = connection
        .transaction()
        .expect("initialization transaction should open");
    let error = initialize(&transaction, "scope", budget(1_048_576, 2), 1)
        .expect_err("two control mutations leave no budget for a reference update");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
    transaction
        .rollback()
        .expect("rejected initialization should roll back");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_reference_resolution_progress",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("progress should count"),
        0
    );
}

#[test]
fn reference_resolution_zero_count_rejects_existing_facts_before_writing() {
    let mut connection = reference_database();
    seed_single_reference(&mut connection);
    let transaction = connection
        .transaction()
        .expect("initialization transaction should open");
    let error = initialize(&transaction, "scope", budget(1_048_576, 3), 0)
        .expect_err("zero frozen count must prove that the reference table is empty");
    assert!(matches!(error, StorageError::Invariant(_)));
    transaction
        .rollback()
        .expect("rejected zero-count initialization should roll back");
    assert_eq!(stale_reference_count(&connection), 1);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_reference_resolution_progress",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("progress should count"),
        0
    );
}

#[test]
fn reference_resolution_page_rejects_enlarged_persisted_limits_before_writing() {
    let mut connection = reference_database();
    seed_single_reference(&mut connection);
    let durable_budget = budget(1_048_576, 3);
    initialize_and_commit(&mut connection, durable_budget, 1);
    connection
        .execute(
            "UPDATE code_repository_reference_resolution_progress
             SET page_document_limit = 2, page_byte_limit = 1048577
             WHERE source_scope = 'scope'",
            [],
        )
        .expect("corrupted enlarged limits should seed");
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(&transaction, "scope", cursor(0, 0, None), durable_budget, 1)
        .expect_err("persisted limits must equal the durable budget derivation");
    assert!(matches!(error, StorageError::Invariant(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    let reference = connection
        .query_row(
            "SELECT target_symbol_snapshot_id, resolution_state
             FROM code_repository_references WHERE reference_id = 'reference:01'",
            [],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("reference should load");
    assert_eq!(reference, (Some("stale".to_owned()), "resolved".to_owned()));
    assert_progress(&connection, 0, None, 0, 0);
}

#[test]
fn reference_resolution_page_rejects_progress_count_detached_from_frozen_checkpoint() {
    let mut connection = hot_owner_database(2);
    let durable_budget = budget(1_048_576, 3);
    initialize_and_commit(&mut connection, durable_budget, 2);
    connection
        .execute(
            "UPDATE code_repository_reference_resolution_progress
             SET expected_reference_count = 0 WHERE source_scope = 'scope'",
            [],
        )
        .expect("detached expected count should seed");
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(&transaction, "scope", cursor(0, 0, None), durable_budget, 2)
        .expect_err("progress expected count must equal the frozen checkpoint count");
    assert!(matches!(error, StorageError::Invariant(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    assert_eq!(stale_reference_count(&connection), 2);
}

#[test]
fn reference_resolution_page_rejects_high_existing_cursor_without_skipping_tail() {
    let mut connection = hot_owner_database(2);
    let durable_budget = budget(1_048_576, 3);
    initialize_and_commit(&mut connection, durable_budget, 2);
    connection
        .execute(
            "UPDATE code_repository_reference_resolution_progress
             SET completed_page_ordinal = 1, cursor_reference_id = 'reference:01',
                 resolved_reference_count = 1
             WHERE source_scope = 'scope'",
            [],
        )
        .expect("high but existing cursor should seed");
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(
        &transaction,
        "scope",
        cursor(1, 1, Some("reference:01")),
        durable_budget,
        2,
    )
    .expect_err("count-bound checkpoint must reject a cursor that skips the tail");
    assert!(matches!(error, StorageError::Invariant(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    assert_eq!(stale_reference_count(&connection), 2);
    assert_progress(&connection, 1, Some("reference:01"), 1, 1);
}

#[test]
fn reference_resolution_page_rejects_progress_cursor_digest_drift_without_writing() {
    let mut connection = reference_database();
    {
        let transaction = connection
            .transaction()
            .expect("fixture transaction should open");
        insert_reference(&transaction, "reference:01", "src/a.rs", "A", "read");
        insert_reference(&transaction, "reference:02", "src/b.rs", "B", "read");
        transaction.commit().expect("fixture should commit");
    }
    let durable_budget = budget(1_048_576, 3);
    initialize_and_commit(&mut connection, durable_budget, 2);
    assert_eq!(
        advance_and_commit(&mut connection, cursor(0, 0, None), durable_budget, 2),
        pending(1, 1, Some("reference:01"))
    );
    connection
        .execute(
            "UPDATE code_repository_reference_resolution_progress
             SET cursor_reference_id = 'reference:02' WHERE source_scope = 'scope'",
            [],
        )
        .expect("progress-only cursor drift should seed");
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(
        &transaction,
        "scope",
        cursor(1, 1, Some("reference:01")),
        durable_budget,
        2,
    )
    .expect_err("checkpoint digest must reject progress-only cursor drift");
    assert!(matches!(error, StorageError::Invariant(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    assert_progress(&connection, 1, Some("reference:02"), 1, 1);
    assert_eq!(stale_reference_count(&connection), 1);
}

#[test]
fn reference_resolution_rejects_oversized_payloads_before_materializing_them() {
    let _trace_test = REFERENCE_SQL_TRACE_TEST
        .lock()
        .expect("trace test should serialize");
    let mut long_name = reference_database();
    {
        let transaction = long_name
            .transaction()
            .expect("fixture transaction should open");
        insert_reference(
            &transaction,
            "reference:huge-name",
            "src/huge.rs",
            &"n".repeat(8 * 1024),
            "read",
        );
        transaction.commit().expect("fixture should commit");
    }
    let tiny_budget = budget(4 * 1024, 3);
    initialize_and_commit(&mut long_name, tiny_budget, 1);
    traced_rejected_page(&mut long_name, tiny_budget);
    let trace = REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .clone();
    assert!(
        !trace
            .iter()
            .any(|sql| sql.contains("SELECT reference_id, path, name")),
        "oversized name/path payload was point-fetched before byte rejection: {trace:?}"
    );

    let mut long_owner = reference_database();
    {
        let transaction = long_owner
            .transaction()
            .expect("fixture transaction should open");
        insert_reference(&transaction, "reference:01", "src/a.rs", "A", "read");
        insert_symbol(&transaction, &"s".repeat(8 * 1024), "src/a.rs", "A");
        transaction.commit().expect("fixture should commit");
    }
    initialize_and_commit(&mut long_owner, tiny_budget, 1);
    traced_rejected_page(&mut long_owner, tiny_budget);
    let trace = REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .clone();
    assert!(
        trace
            .iter()
            .any(|sql| sql.contains("length(CAST(symbol_snapshot_id AS BLOB))")),
        "owner admission did not use the length-only probe: {trace:?}"
    );
    assert!(
        !trace
            .iter()
            .any(|sql| sql.trim_start().starts_with("SELECT symbol_snapshot_id")),
        "oversized owner id was materialized before byte rejection: {trace:?}"
    );
}

#[test]
fn code_index_persistence_performance_suite_reference_resolution_uses_static_keysets_and_two_owner_probes()
 {
    let connection = reference_database();
    for sql in [
        paged_sql::SCAN_FIRST,
        paged_sql::SCAN_AFTER,
        paged_sql::UPDATE_FIRST,
        paged_sql::UPDATE_AFTER,
    ] {
        assert!(
            !sql.contains("IS NULL OR"),
            "nullable-OR range returned: {sql}"
        );
        assert!(
            !sql.contains("COUNT("),
            "unbounded owner count returned: {sql}"
        );
        assert!(
            !sql.contains("MIN("),
            "unbounded owner aggregate returned: {sql}"
        );
    }
    for sql in [paged_sql::NAME_OWNERS, paged_sql::PATH_OWNERS] {
        assert!(sql.contains("LIMIT 2"));
        assert!(!sql.contains("COUNT("));
    }
    for sql in [paged_sql::UPDATE_FIRST, paged_sql::UPDATE_AFTER] {
        assert_eq!(sql.matches("LIMIT 1 OFFSET 1").count(), 2);
        assert_eq!(
            sql.matches("kind != 'call'").count(),
            1,
            "call exclusion must happen before the limited CTE materializes payload: {sql}"
        );
    }

    let plan_first = explain(
        &connection,
        paged_sql::SCAN_FIRST,
        vec![Value::Text("scope".to_owned()), 8.into()],
    );
    let plan_after = explain(
        &connection,
        paged_sql::SCAN_AFTER,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference:00".to_owned()),
            8.into(),
        ],
    );
    let name_probe = explain(
        &connection,
        paged_sql::NAME_OWNERS,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("Hot".to_owned()),
        ],
    );
    let path_probe = explain(
        &connection,
        paged_sql::PATH_OWNERS,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("Hot".to_owned()),
            Value::Text("src/hot.rs".to_owned()),
        ],
    );
    let update_first = explain(
        &connection,
        paged_sql::UPDATE_FIRST,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference:01".to_owned()),
        ],
    );
    let update_after = explain(
        &connection,
        paged_sql::UPDATE_AFTER,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference:00".to_owned()),
            Value::Text("reference:01".to_owned()),
        ],
    );
    assert_reference_keyset(&plan_first, "source_scope=?");
    assert_reference_keyset(&plan_after, "reference_id>?");
    assert_reference_keyset(&update_first, "reference_id<?");
    assert_reference_keyset(&update_after, "reference_id>? AND reference_id<?");
    for details in [&name_probe, &path_probe, &update_first, &update_after] {
        let joined = details.join("\n");
        assert!(
            details
                .iter()
                .filter(|detail| detail.contains("code_repository_symbols_name_path_lookup"))
                .count()
                >= 1,
            "bounded owner probes must use the owner index:\n{joined}"
        );
    }
}

#[test]
fn code_index_persistence_performance_suite_reference_resolution_vm_steps_ignore_tenfold_hot_owner_tail()
 {
    let (small_first_plan, small_first_update, small_after_plan, small_after_update) =
        hot_owner_vm_steps(256);
    let (large_first_plan, large_first_update, large_after_plan, large_after_update) =
        hot_owner_vm_steps(2_560);
    eprintln!(
        "REFERENCE_RESOLUTION_VM_STEPS small_plan={small_first_plan}/{small_after_plan} \
         large_plan={large_first_plan}/{large_after_plan} small_update={small_first_update}/{small_after_update} \
         large_update={large_first_update}/{large_after_update}"
    );
    for (small, large, label) in [
        (small_first_plan, large_first_plan, "first plan"),
        (small_first_update, large_first_update, "first update"),
        (small_after_plan, large_after_plan, "continuation plan"),
        (
            small_after_update,
            large_after_update,
            "continuation update",
        ),
    ] {
        assert!(
            large <= small + 64,
            "{label} VM work scaled with the tenfold duplicate-symbol tail: small={small}, large={large}"
        );
    }
}

pub(super) fn budget(
    max_bytes_per_batch: usize,
    max_rows_per_batch: usize,
) -> CodeIndexResourceBudget {
    CodeIndexResourceBudget::new(1, max_bytes_per_batch, max_rows_per_batch)
        .expect("test budget should be valid")
}

pub(super) fn cursor(
    completed_page_ordinal: usize,
    completed_reference_count: usize,
    cursor_reference_id: Option<&str>,
) -> CodeReferenceResolution {
    CodeReferenceResolution {
        protocol_version: 1,
        stage: CodeReferenceResolutionStage::Resolve,
        completed_page_ordinal,
        completed_reference_count,
        cursor_digest: code_reference_resolution_cursor_digest(cursor_reference_id),
    }
}

pub(super) fn pending(
    completed_page_ordinal: usize,
    completed_reference_count: usize,
    cursor_reference_id: Option<&str>,
) -> ReferenceResolutionAdvance {
    ReferenceResolutionAdvance::Pending {
        completed_page_ordinal,
        completed_reference_count,
        cursor_reference_id: cursor_reference_id.map(str::to_owned),
    }
}

pub(super) fn initialize_and_commit(
    connection: &mut Connection,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) {
    let transaction = connection
        .transaction()
        .expect("initialization transaction should open");
    assert_eq!(
        initialize(
            &transaction,
            "scope",
            resource_budget,
            expected_reference_count,
        )
        .expect("progress should initialize"),
        pending(0, 0, None)
    );
    transaction.commit().expect("initialization should commit");
}

pub(super) fn advance_and_commit(
    connection: &mut Connection,
    checkpoint: CodeReferenceResolution,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> ReferenceResolutionAdvance {
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let advance = advance(
        &transaction,
        "scope",
        checkpoint,
        resource_budget,
        expected_reference_count,
    )
    .expect("page should advance");
    transaction.commit().expect("page should commit");
    advance
}

fn assert_progress(
    connection: &Connection,
    page: usize,
    cursor: Option<&str>,
    resolved: usize,
    last_page_rows: usize,
) {
    let persisted = connection
        .query_row(
            "SELECT completed_page_ordinal, cursor_reference_id, resolved_reference_count
             FROM code_repository_reference_resolution_progress WHERE source_scope = 'scope'",
            [],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .expect("progress should remain");
    assert_eq!(persisted, (page, cursor.map(str::to_owned), resolved));
    if page > 0 {
        let prior = if page == 1 {
            0
        } else if page == 2 {
            2
        } else {
            4
        };
        assert_eq!(resolved - prior, last_page_rows);
    }
}

fn first_row_bytes(connection: &mut Connection) -> usize {
    let transaction = connection
        .transaction()
        .expect("row-size transaction should open");
    let control_bytes = page_control_bytes(&transaction, "scope")
        .expect("control bytes should derive from the checkpoint");
    let (repository_id, source_scope, reference_id, file_id, path, name, kind, target_id) =
        transaction
            .query_row(
                "SELECT reference.repository_id, reference.source_scope,
                    reference.reference_id, reference.file_id, reference.path,
                    reference.name, reference.kind, symbol.symbol_snapshot_id
             FROM code_repository_references reference
             INNER JOIN code_repository_symbols symbol
               ON symbol.source_scope = reference.source_scope
              AND symbol.name = reference.name
             WHERE reference.source_scope = 'scope'
             ORDER BY reference.reference_id LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .expect("independent row fixture should load");
    let bytes = control_bytes
        + repository_id.len()
        + source_scope.len()
        + reference_id.len()
        + file_id.len()
        + path.len()
        + 2 * name.len()
        + kind.len()
        + target_id.len()
        + "resolved".len()
        + "inferred".len()
        + 153
        + reference_id.len();
    transaction
        .rollback()
        .expect("row-size transaction should roll back");
    bytes
}

type ResolutionRow = (String, Option<String>, Option<String>, String, u16, String);

fn resolution_rows(connection: &Connection) -> Vec<ResolutionRow> {
    connection
        .prepare(
            "SELECT reference_id, target_symbol_snapshot_id, target_hint,
                    resolution_state, confidence_basis_points, confidence_tier
             FROM code_repository_references ORDER BY reference_id",
        )
        .expect("resolution query should prepare")
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("resolution rows should query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("resolution rows should collect")
}

pub(super) fn stale_reference_count(connection: &Connection) -> usize {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_references
             WHERE target_symbol_snapshot_id = 'stale' AND resolution_state = 'resolved'",
            [],
            |row| row.get(0),
        )
        .expect("stale references should count")
}

fn explain(connection: &Connection, sql: &str, values: Vec<Value>) -> Vec<String> {
    connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("query plan should prepare")
        .query_map(params_from_iter(values), |row| row.get(3))
        .expect("query plan should execute")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("query plan should collect")
}

fn assert_reference_keyset(details: &[String], expected_range: &str) {
    let joined = details.join("\n");
    assert!(
        details.iter().any(|detail| {
            detail.contains("code_repository_references")
                && detail.contains(expected_range)
                && detail.contains("INDEX")
        }),
        "expected reference keyset '{expected_range}', got:\n{joined}"
    );
}

fn hot_owner_vm_steps(duplicate_count: usize) -> (usize, usize, usize, usize) {
    let mut first_plan = hot_owner_database(duplicate_count);
    let first_plan = measured_vm_steps(&mut first_plan, |transaction| {
        execute_scan_and_owner_probes(transaction, false);
    });
    let mut first_update = hot_owner_database(duplicate_count);
    let first_update = measured_vm_steps(&mut first_update, |transaction| {
        assert_eq!(
            transaction
                .execute(paged_sql::UPDATE_FIRST, params!["scope", "reference:00"],)
                .expect("first update should execute"),
            1
        );
    });
    let mut after_plan = hot_owner_database(duplicate_count);
    let after_plan = measured_vm_steps(&mut after_plan, |transaction| {
        execute_scan_and_owner_probes(transaction, true);
    });
    let mut after_update = hot_owner_database(duplicate_count);
    let after_update = measured_vm_steps(&mut after_update, |transaction| {
        assert_eq!(
            transaction
                .execute(
                    paged_sql::UPDATE_AFTER,
                    params!["scope", "reference:00", "reference:01"],
                )
                .expect("continuation update should execute"),
            1
        );
    });
    (first_plan, first_update, after_plan, after_update)
}

fn execute_scan_and_owner_probes(transaction: &Transaction<'_>, after: bool) {
    let scanned = if after {
        transaction.query_row(
            paged_sql::SCAN_AFTER,
            params!["scope", "reference:00", 1],
            |row| row.get::<_, i64>(0),
        )
    } else {
        transaction.query_row(paged_sql::SCAN_FIRST, params!["scope", 1], |row| {
            row.get::<_, i64>(0)
        })
    }
    .expect("streaming scan should return one row");
    assert!(scanned > 0);
    for (sql, values) in [
        (paged_sql::NAME_OWNERS, vec!["scope", "Hot"]),
        (paged_sql::PATH_OWNERS, vec!["scope", "Hot", "src/hot.rs"]),
    ] {
        let mut statement = transaction.prepare(sql).expect("probe should prepare");
        let owners = statement
            .query_map(params_from_iter(values), |row| row.get::<_, usize>(0))
            .expect("probe should execute")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("probe owners should collect");
        assert_eq!(owners.len(), 2);
    }
}

fn traced_rejected_page(connection: &mut Connection, resource_budget: CodeIndexResourceBudget) {
    REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .clear();
    connection.trace(Some(capture_reference_sql));
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let error = advance(
        &transaction,
        "scope",
        cursor(0, 0, None),
        resource_budget,
        1,
    )
    .expect_err("oversized payload must reject the page");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
    transaction
        .rollback()
        .expect("rejected page should roll back");
    connection.trace(None);
}

pub(super) fn capture_reference_sql(sql: &str) {
    REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .push(sql.to_owned());
}

fn measured_vm_steps(
    connection: &mut Connection,
    operation: impl FnOnce(&Transaction<'_>),
) -> usize {
    let steps = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&steps);
    connection.progress_handler(
        1,
        Some(move || {
            observed.fetch_add(1, Ordering::Relaxed);
            false
        }),
    );
    let transaction = connection
        .transaction()
        .expect("measurement transaction should open");
    operation(&transaction);
    transaction
        .rollback()
        .expect("measurement should roll back");
    connection.progress_handler(0, None::<fn() -> bool>);
    steps.load(Ordering::Relaxed)
}

fn hot_owner_database(duplicate_count: usize) -> Connection {
    let mut connection = reference_database();
    let transaction = connection
        .transaction()
        .expect("fixture transaction should open");
    insert_reference(&transaction, "reference:00", "src/hot.rs", "Hot", "read");
    insert_reference(&transaction, "reference:01", "src/hot.rs", "Hot", "read");
    for ordinal in 0..duplicate_count {
        insert_symbol(
            &transaction,
            &format!("symbol:{ordinal:05}"),
            "src/hot.rs",
            "Hot",
        );
    }
    transaction.commit().expect("fixture should commit");
    connection
}

fn seed_single_reference(connection: &mut Connection) {
    let transaction = connection
        .transaction()
        .expect("fixture transaction should open");
    insert_reference(&transaction, "reference:01", "src/only.rs", "Only", "read");
    insert_symbol(&transaction, "symbol:only", "src/only.rs", "Only");
    transaction.commit().expect("fixture should commit");
}

fn seed_semantic_fixture(connection: &mut Connection) {
    let transaction = connection
        .transaction()
        .expect("fixture transaction should open");
    insert_reference(&transaction, "reference:01", "src/a.rs", "Widget", "read");
    insert_reference(
        &transaction,
        "reference:02",
        "src/other.rs",
        "Widget",
        "read",
    );
    insert_reference(&transaction, "reference:03", "src/use.rs", "Only", "read");
    insert_reference(
        &transaction,
        "reference:04",
        "src/use.rs",
        "Missing",
        "read",
    );
    insert_reference(&transaction, "reference:05", "src/use.rs", "Only", "call");
    insert_symbol(&transaction, "symbol:a", "src/a.rs", "Widget");
    insert_symbol(&transaction, "symbol:b", "src/b.rs", "Widget");
    insert_symbol(&transaction, "symbol:only", "src/only.rs", "Only");
    transaction.commit().expect("fixture should commit");
}

pub(super) fn insert_reference(
    transaction: &Transaction<'_>,
    reference_id: &str,
    path: &str,
    name: &str,
    kind: &str,
) {
    transaction
        .execute(
            "INSERT INTO code_repository_references (
                 repository_id, source_scope, reference_id, file_id, path, name, kind,
                 target_symbol_snapshot_id, target_hint, resolution_state,
                 confidence_basis_points, confidence_tier, byte_start, byte_end,
                 line_start, line_end
             ) VALUES (
                 'repo', 'scope', ?1, 'file', ?2, ?3, ?4,
                 'stale', 'stale', 'resolved', 9999, 'exact', 0, 1, 1, 1
             )",
            params![reference_id, path, name, kind],
        )
        .expect("reference should insert");
}

fn insert_symbol(transaction: &Transaction<'_>, symbol_id: &str, path: &str, name: &str) {
    transaction
        .execute(
            "INSERT INTO code_repository_symbols (
                 source_scope, symbol_snapshot_id, path, name
             ) VALUES ('scope', ?1, ?2, ?3)",
            params![symbol_id, path, name],
        )
        .expect("symbol should insert");
}

pub(super) fn reference_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_references (
                 repository_id TEXT NOT NULL, source_scope TEXT NOT NULL,
                 reference_id TEXT NOT NULL, file_id TEXT NOT NULL, path TEXT NOT NULL,
                 name TEXT NOT NULL, kind TEXT NOT NULL,
                 target_symbol_snapshot_id TEXT, target_hint TEXT,
                 resolution_state TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL,
                 confidence_tier TEXT NOT NULL, byte_start INTEGER NOT NULL,
                 byte_end INTEGER NOT NULL, line_start INTEGER NOT NULL, line_end INTEGER NOT NULL,
                 PRIMARY KEY (source_scope, reference_id)
             );
             CREATE TABLE code_repository_symbols (
                 source_scope TEXT NOT NULL, symbol_snapshot_id TEXT NOT NULL,
                 path TEXT NOT NULL, name TEXT NOT NULL,
                 PRIMARY KEY (source_scope, symbol_snapshot_id)
             );
             CREATE INDEX code_repository_symbols_name_path_lookup
                 ON code_repository_symbols(source_scope, name, path);
             CREATE TABLE code_repository_index_checkpoints (
                 source_scope TEXT PRIMARY KEY, repository_id TEXT NOT NULL,
                 state TEXT NOT NULL, resolved_commit_sha TEXT NOT NULL,
                 tree_hash TEXT NOT NULL, path_filters_json TEXT NOT NULL,
                 language_filters_json TEXT NOT NULL, total_path_count INTEGER NOT NULL,
                 parsed_file_count INTEGER NOT NULL, committed_file_count INTEGER NOT NULL,
                 committed_symbol_count INTEGER NOT NULL,
                 committed_reference_count INTEGER NOT NULL,
                 committed_chunk_count INTEGER NOT NULL, batch_count INTEGER NOT NULL,
                 last_path TEXT, resource_budget_json TEXT NOT NULL,
                 updated_at_ms INTEGER NOT NULL, error_message TEXT
             );
             INSERT INTO code_repository_index_checkpoints VALUES (
                 'scope', 'repo', 'finalizing:build_query_indexes', 'commit', 'tree',
                 '[]', '[]', 1, 1, 1, 1, 5, 0, 1, NULL,
                 '{\"max_files_per_batch\":1,\"max_bytes_per_batch\":1048576,\"max_rows_per_batch\":4}',
                 1, NULL
             );
             CREATE TABLE code_repository_reference_resolution_progress (
                 source_scope TEXT NOT NULL PRIMARY KEY,
                 protocol_version INTEGER NOT NULL, stage TEXT NOT NULL,
                 completed_page_ordinal INTEGER NOT NULL, cursor_reference_id TEXT,
                 expected_reference_count INTEGER NOT NULL,
                 resolved_reference_count INTEGER NOT NULL,
                 page_document_limit INTEGER NOT NULL, page_byte_limit INTEGER NOT NULL
             );",
        )
        .expect("reference schema should initialize");
    connection
}
