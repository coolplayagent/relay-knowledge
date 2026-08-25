//! Direct plan-shape and VM-work regressions for grouped reference-search pages.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use super::{
    PagePlan, Progress, discovery_page_plan, insert_group_search_page, sql, upsert_discovery_page,
};
use crate::domain::CodeReferenceSearchRebuildStage;

static GROUPED_SQL_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());
static GROUPED_SQL_TRACE_TEST: Mutex<()> = Mutex::new(());

const LEGACY_DISCOVERY_PLAN: &str = "WITH limited AS (
         SELECT reference.reference_id,
                length(CAST(reference.source_scope AS BLOB))
                + length(CAST(reference.reference_id AS BLOB))
                + length(CAST(reference.name AS BLOB))
                + length(CAST(reference.kind AS BLOB))
                + length(CAST(reference.path AS BLOB))
                + length(CAST(coalesce(reference.target_hint, '') AS BLOB))
                + length(CAST(coalesce(file.language_id, '') AS BLOB)) + 8 AS row_bytes
         FROM code_repository_references reference
         LEFT JOIN code_repository_files file
           ON file.source_scope = reference.source_scope AND file.path = reference.path
         WHERE reference.source_scope = ?1
           AND (?2 IS NULL OR reference.reference_id > ?2)
         ORDER BY reference.reference_id LIMIT ?3
     ), sized AS (
         SELECT reference_id AS cursor, row_bytes,
                row_number() OVER (ORDER BY reference_id) AS ordinal,
                sum(row_bytes) OVER (ORDER BY reference_id) AS cumulative_bytes
         FROM limited
     )
     SELECT coalesce(sum(CASE WHEN cumulative_bytes <= ?4 THEN 1 ELSE 0 END), 0),
            max(CASE WHEN cumulative_bytes <= ?4 THEN cursor END),
            max(CASE WHEN ordinal = 1 THEN row_bytes END)
     FROM sized";
const LEGACY_DISCOVERY_GROUP_COUNTS: &str = "WITH page_groups AS (
         SELECT reference.name, reference.kind, reference.path,
                coalesce(reference.target_hint, '') AS target_hint
         FROM code_repository_references reference
         WHERE reference.source_scope = ?1
           AND (?2 IS NULL OR reference.reference_id > ?2)
           AND reference.reference_id <= ?3
         GROUP BY reference.name, reference.kind, reference.path,
                  coalesce(reference.target_hint, '')
     )
     SELECT COUNT(*), coalesce(SUM(NOT EXISTS (
                SELECT 1 FROM code_repository_reference_search_groups existing
                WHERE existing.source_scope = ?1
                  AND existing.name = page_groups.name
                  AND existing.kind = page_groups.kind
                  AND existing.path = page_groups.path
                  AND existing.target_hint = page_groups.target_hint
            )), 0)
     FROM page_groups";
const LEGACY_DISCOVERY_UPSERT: &str = "INSERT INTO code_repository_reference_search_groups (
         source_scope, group_id, name, kind, path, target_hint, language_id, occurrence_count
     )
     SELECT ?1, MIN(reference.reference_id), reference.name, reference.kind,
            reference.path, coalesce(reference.target_hint, ''),
            coalesce(file.language_id, ''), COUNT(*)
     FROM code_repository_references reference
     LEFT JOIN code_repository_files file
       ON file.source_scope = reference.source_scope AND file.path = reference.path
     WHERE reference.source_scope = ?1
       AND (?2 IS NULL OR reference.reference_id > ?2)
       AND reference.reference_id <= ?3
     GROUP BY reference.name, reference.kind, reference.path,
              coalesce(reference.target_hint, ''), coalesce(file.language_id, '')
     ON CONFLICT (source_scope, name, kind, path, target_hint) DO UPDATE SET
         group_id = min(group_id, excluded.group_id),
         language_id = excluded.language_id,
         occurrence_count = occurrence_count + excluded.occurrence_count";

#[test]
fn code_index_persistence_performance_suite_grouped_range_sql_uses_keyset_plans() {
    let connection = grouped_database();
    for statement in all_page_statements() {
        assert!(
            !statement.contains("IS NULL OR"),
            "grouped page SQL must not restore a nullable-OR range: {statement}"
        );
    }

    let discovery_first = explain(
        &connection,
        sql::DISCOVERY_SCAN_FIRST,
        vec![Value::Text("scope".to_owned()), 512.into()],
    );
    let discovery_after = explain(
        &connection,
        sql::DISCOVERY_SCAN_AFTER,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference-0511".to_owned()),
            512.into(),
        ],
    );
    assert_keyset_plan(&discovery_first, "reference", "source_scope=?");
    assert_keyset_plan(&discovery_after, "reference", "reference_id>?");

    let cleanup_first = explain(
        &connection,
        sql::CLEANUP_SCAN_FIRST,
        vec![Value::Text("scope".to_owned()), 512.into()],
    );
    let cleanup_after = explain(
        &connection,
        sql::CLEANUP_SCAN_AFTER,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference-0511".to_owned()),
            512.into(),
        ],
    );
    assert_keyset_plan(&cleanup_first, "metadata", "source_scope=?");
    assert_keyset_plan(&cleanup_after, "metadata", "record_id>?");

    let build_first = explain(
        &connection,
        sql::BUILD_SCAN_FIRST,
        vec![Value::Text("scope".to_owned()), 512.into()],
    );
    let build_after = explain(
        &connection,
        sql::BUILD_SCAN_AFTER,
        vec![
            Value::Text("scope".to_owned()),
            Value::Text("reference-0063".to_owned()),
            512.into(),
        ],
    );
    assert_keyset_plan(&build_first, "search_group", "source_scope=?");
    assert_keyset_plan(&build_after, "search_group", "group_id>?");
}

#[test]
fn code_index_persistence_performance_suite_merged_discovery_reduces_vm_steps() {
    let mut legacy = grouped_database();
    let mut merged = grouped_database();
    seed_discovery_fixture(&mut legacy);
    seed_discovery_fixture(&mut merged);
    let progress = continuation_progress();

    let legacy_steps = measured_vm_steps(&mut legacy, |transaction| {
        let plan = transaction
            .query_row(
                LEGACY_DISCOVERY_PLAN,
                params!["scope", "reference-0511", 512, 1_048_576],
                |row| {
                    Ok(PagePlan::<String> {
                        row_count: row.get(0)?,
                        last_cursor: row.get(1)?,
                        first_row_bytes: row.get(2)?,
                    })
                },
            )
            .expect("legacy page plan should execute");
        assert_eq!(plan.row_count, 512);
        assert_eq!(plan.last_cursor.as_deref(), Some("reference-1023"));
        let counts = transaction
            .query_row(
                LEGACY_DISCOVERY_GROUP_COUNTS,
                params!["scope", "reference-0511", "reference-1023"],
                |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
            )
            .expect("legacy group counts should execute");
        assert_eq!(counts, (128, 0));
        assert_eq!(
            transaction
                .execute(
                    LEGACY_DISCOVERY_UPSERT,
                    params!["scope", "reference-0511", "reference-1023"],
                )
                .expect("legacy upsert should execute"),
            128
        );
    });
    let merged_steps = measured_vm_steps(&mut merged, |transaction| {
        let plan = discovery_page_plan(transaction, "scope", &progress)
            .expect("static continuation plan should execute");
        assert_eq!(plan.row_count, 512);
        assert_eq!(plan.last_cursor.as_deref(), Some("reference-1023"));
        assert_eq!(
            upsert_discovery_page(transaction, "scope", &progress, "reference-1023")
                .expect("merged upsert should execute"),
            (128, 0)
        );
    });

    eprintln!(
        "REFERENCE_DISCOVERY_VM_STEPS legacy={legacy_steps} merged={merged_steps} reduction={}",
        legacy_steps.saturating_sub(merged_steps)
    );
    assert!(
        merged_steps < legacy_steps,
        "merged discovery must execute fewer SQLite VM steps than nullable-OR plus duplicate GROUP BY"
    );
}

#[test]
fn code_index_persistence_performance_suite_grouped_stream_rejects_huge_cursor_before_fetch() {
    let _trace_test = GROUPED_SQL_TRACE_TEST
        .lock()
        .expect("trace test should serialize");
    let mut connection = grouped_database();
    let huge_reference_id = "r".repeat(8 * 1024);
    connection
        .execute(
            "INSERT INTO code_repository_files (source_scope, path, language_id)
             VALUES ('scope', 'src/huge.rs', 'rust')",
            [],
        )
        .expect("file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_references
             (source_scope, reference_id, name, kind, path, target_hint)
             VALUES ('scope', ?1, 'Huge', 'read', 'src/huge.rs', 'Huge')",
            [&huge_reference_id],
        )
        .expect("reference should insert");
    let progress = Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Discover,
        completed_page_ordinal: 0,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: None,
        build_cursor_group_id: None,
        expected_reference_count: 1,
        cleanup_total_count: 0,
        discovered_reference_count: 0,
        discovered_group_count: 0,
        build_total_count: 0,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: 1,
        page_byte_limit: 4_096,
    };

    GROUPED_SQL_TRACE.lock().expect("trace should lock").clear();
    connection.trace(Some(capture_grouped_sql));
    let transaction = connection
        .transaction()
        .expect("page transaction should open");
    let plan = discovery_page_plan(&transaction, "scope", &progress)
        .expect("length-only stream should plan without materializing the cursor");
    assert_eq!(plan.row_count, 0);
    assert!(plan.last_cursor.is_none());
    assert!(plan.first_row_bytes.is_some_and(|bytes| bytes > 4_096));
    transaction
        .rollback()
        .expect("measurement should roll back");
    connection.trace(None);

    let trace = GROUPED_SQL_TRACE.lock().expect("trace should lock").clone();
    assert!(
        trace.iter().all(|statement| {
            let statement = statement.to_ascii_lowercase();
            !(statement.contains("select reference_id") && statement.contains("rowid ="))
        }),
        "an over-budget cursor must be rejected before payload fetch: {trace:?}"
    );

    let mut build_connection = grouped_database();
    let huge_name = "N".repeat(8 * 1024);
    build_connection
        .execute(
            "INSERT INTO code_repository_reference_search_groups (
                 source_scope, group_id, name, kind, path, target_hint,
                 language_id, occurrence_count
             ) VALUES ('scope', 'group:huge', ?1, 'read', 'src/huge.rs',
                       'Hint', 'rust', 1)",
            [&huge_name],
        )
        .expect("large group should insert");
    let build_progress = Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Build,
        completed_page_ordinal: 0,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: None,
        build_cursor_group_id: None,
        expected_reference_count: 1,
        cleanup_total_count: 0,
        discovered_reference_count: 1,
        discovered_group_count: 1,
        build_total_count: 1,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: 1,
        page_byte_limit: 4_096,
    };
    GROUPED_SQL_TRACE.lock().expect("trace should lock").clear();
    build_connection.trace(Some(capture_grouped_sql));
    let transaction = build_connection
        .transaction()
        .expect("build transaction should open");
    let plan = super::build_page_plan(&transaction, "scope", &build_progress)
        .expect("length algebra should reject without constructing search content");
    assert_eq!(plan.row_count, 0);
    assert!(plan.last_cursor.is_none());
    assert!(plan.first_row_bytes.is_some_and(|bytes| bytes > 4_096));
    transaction
        .rollback()
        .expect("measurement should roll back");
    build_connection.trace(None);
    let trace = GROUPED_SQL_TRACE.lock().expect("trace should lock").clone();
    assert!(
        trace.iter().all(|statement| {
            let statement = statement.to_ascii_lowercase();
            !(statement.contains("select group_id") && statement.contains("rowid ="))
        }),
        "an over-budget build payload must be rejected before cursor/content fetch: {trace:?}"
    );
    assert!(
        !sql::BUILD_SCAN_FIRST.contains("||") && !sql::BUILD_SCAN_AFTER.contains("||"),
        "build admission must add field lengths instead of concatenating content"
    );
}

#[test]
fn code_index_persistence_performance_suite_grouped_stream_fetches_only_the_final_cursor() {
    let _trace_test = GROUPED_SQL_TRACE_TEST
        .lock()
        .expect("trace test should serialize");
    let mut connection = grouped_database();
    connection
        .execute(
            "INSERT INTO code_repository_files (source_scope, path, language_id)
             VALUES ('scope', 'src/lib.rs', 'rust')",
            [],
        )
        .expect("file should insert");
    {
        let transaction = connection
            .transaction()
            .expect("seed transaction should start");
        let mut insert = transaction
            .prepare(
                "INSERT INTO code_repository_references
                 (source_scope, reference_id, name, kind, path, target_hint)
                 VALUES ('scope', ?1, ?2, 'read', 'src/lib.rs', ?2)",
            )
            .expect("reference insert should prepare");
        for index in 0..1_025 {
            insert
                .execute(params![
                    format!("reference-{index:04}"),
                    format!("Target{index:04}"),
                ])
                .expect("reference should insert");
        }
        drop(insert);
        transaction.commit().expect("seed should commit");
    }
    let progress = Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Discover,
        completed_page_ordinal: 0,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: None,
        build_cursor_group_id: None,
        expected_reference_count: 1_025,
        cleanup_total_count: 0,
        discovered_reference_count: 0,
        discovered_group_count: 0,
        build_total_count: 0,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: 1_024,
        page_byte_limit: 1_048_576,
    };

    GROUPED_SQL_TRACE.lock().expect("trace should lock").clear();
    connection.trace(Some(capture_grouped_sql));
    let transaction = connection
        .transaction()
        .expect("page transaction should start");
    let plan =
        discovery_page_plan(&transaction, "scope", &progress).expect("discovery page should plan");
    assert_eq!(plan.row_count, 1_024);
    assert_eq!(plan.last_cursor.as_deref(), Some("reference-1023"));
    transaction
        .rollback()
        .expect("measurement should roll back");
    connection.trace(None);

    let trace = GROUPED_SQL_TRACE.lock().expect("trace should lock").clone();
    let cursor_fetches = trace
        .iter()
        .filter(|statement| {
            let statement = statement.to_ascii_lowercase();
            statement.contains("select reference_id") && statement.contains("rowid =")
        })
        .count();
    assert_eq!(
        cursor_fetches, 1,
        "one admitted page must fetch only its final durable cursor: {trace:?}"
    );
}

#[test]
fn code_index_persistence_performance_suite_grouped_build_bulk_inserts_one_admitted_page() {
    let _trace_test = GROUPED_SQL_TRACE_TEST
        .lock()
        .expect("trace test should serialize");
    let mut connection = grouped_database();
    {
        let transaction = connection
            .transaction()
            .expect("seed transaction should start");
        let mut insert = transaction
            .prepare(
                "INSERT INTO code_repository_reference_search_groups (
                     source_scope, group_id, name, kind, path, target_hint,
                     language_id, occurrence_count
                 ) VALUES ('scope', ?1, ?2, 'call', 'src/lib.rs', ?3, 'rust', 1)",
            )
            .expect("group insert should prepare");
        for index in 0..1_025 {
            let target_hint = if index % 2 == 0 { "" } else { "Hint" };
            insert
                .execute(params![
                    format!("group-{index:04}"),
                    format!("Target{index:04}"),
                    target_hint,
                ])
                .expect("group should insert");
        }
        drop(insert);
        transaction.commit().expect("seed should commit");
    }
    let progress = Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Build,
        completed_page_ordinal: 0,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: None,
        build_cursor_group_id: None,
        expected_reference_count: 1_025,
        cleanup_total_count: 0,
        discovered_reference_count: 1_025,
        discovered_group_count: 1_025,
        build_total_count: 1_025,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: 1_025,
        page_byte_limit: 1_048_576,
    };

    GROUPED_SQL_TRACE.lock().expect("trace should lock").clear();
    connection.trace(Some(capture_grouped_sql));
    let transaction = connection
        .transaction()
        .expect("build transaction should start");
    assert_eq!(
        insert_group_search_page(&transaction, "scope", &progress, "group-1024")
            .expect("bulk page should insert"),
        1_025
    );
    assert_eq!(
        transaction
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("FTS count should query"),
        1_025
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata count should query"),
        1_025
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT content FROM code_repository_search WHERE record_id = 'group-0000'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("empty-hint content should query"),
        "Target0000 call src/lib.rs"
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT content FROM code_repository_search WHERE record_id = 'group-0001'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("hint content should query"),
        "Target0001 call Hint src/lib.rs"
    );
    transaction.commit().expect("build should commit");
    connection.trace(None);

    let trace = GROUPED_SQL_TRACE.lock().expect("trace should lock").clone();
    let main_fts_inserts = trace
        .iter()
        .filter(|statement| {
            statement
                .trim_start()
                .starts_with("INSERT INTO code_repository_search (")
        })
        .count();
    let metadata_inserts = trace
        .iter()
        .filter(|statement| {
            statement
                .trim_start()
                .starts_with("INSERT INTO code_repository_search_metadata (")
        })
        .count();
    assert_eq!(main_fts_inserts, 1, "bulk page trace: {trace:?}");
    assert_eq!(metadata_inserts, 1, "metadata trace: {trace:?}");
    assert_eq!(
        trace
            .iter()
            .filter(|statement| statement.contains("rowid = 9223372036854775807"))
            .count(),
        1,
        "bulk page must guard consecutive rowid allocation: {trace:?}"
    );
}

#[test]
fn grouped_bulk_build_matches_the_canonical_reference_content_encoder() {
    let mut connection = grouped_database();
    let fields = [
        ("Name", "call", "Hint", "src/lib.rs"),
        ("Name", "call", "", "src/lib.rs"),
        ("", "call", "Hint", "src/lib.rs"),
        ("Name", "", "Hint", "src/lib.rs"),
        ("Name", "call", "Hint", ""),
        (" ", " ", "Hint", "src/lib.rs"),
    ];
    for (index, (name, kind, target_hint, path)) in fields.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO code_repository_reference_search_groups (
                     source_scope, group_id, name, kind, path, target_hint,
                     language_id, occurrence_count
                 ) VALUES ('scope', ?1, ?2, ?3, ?4, ?5, 'rust', 1)",
                params![format!("group-{index}"), name, kind, path, target_hint],
            )
            .expect("group should insert");
    }
    let progress = build_progress(fields.len());
    let transaction = connection
        .transaction()
        .expect("build transaction should start");
    assert_eq!(
        insert_group_search_page(
            &transaction,
            "scope",
            &progress,
            &format!("group-{}", fields.len() - 1),
        )
        .expect("bulk page should insert"),
        fields.len()
    );
    for (index, (name, kind, target_hint, path)) in fields.iter().enumerate() {
        let content = transaction
            .query_row(
                "SELECT content FROM code_repository_search WHERE record_id = ?1",
                [format!("group-{index}")],
                |row| row.get::<_, String>(0),
            )
            .expect("content should query");
        assert_eq!(
            content,
            crate::storage::sqlite::code::search::search_document_content(
                "reference",
                [*name, *kind, *target_hint, *path],
            )
        );
    }
}

#[test]
fn grouped_bulk_build_rejects_maximum_fts_rowid_before_owner_write() {
    let mut connection = grouped_database();
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (?1, 'orphan', 'chunk', 'orphan', 'orphan', 'rust', 'orphan')",
            [i64::MAX],
        )
        .expect("maximum orphan row should insert");
    connection
        .execute(
            "INSERT INTO code_repository_reference_search_groups (
                 source_scope, group_id, name, kind, path, target_hint,
                 language_id, occurrence_count
             ) VALUES ('scope', 'group-0', 'Name', 'call', 'src/lib.rs', '', 'rust', 1)",
            [],
        )
        .expect("group should insert");
    let transaction = connection
        .transaction()
        .expect("build transaction should start");
    let error = insert_group_search_page(&transaction, "scope", &build_progress(1), "group-0")
        .expect_err("maximum rowid must fail closed");
    assert!(error.to_string().contains("maximum SQLite rowid"));
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata count should query"),
        0
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search WHERE source_scope = 'scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("scope FTS count should query"),
        0
    );
}

fn build_progress(group_count: usize) -> Progress {
    Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Build,
        completed_page_ordinal: 0,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: None,
        build_cursor_group_id: None,
        expected_reference_count: group_count,
        cleanup_total_count: 0,
        discovered_reference_count: group_count,
        discovered_group_count: group_count,
        build_total_count: group_count,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: group_count,
        page_byte_limit: 1_048_576,
    }
}

fn all_page_statements() -> [&'static str; 18] {
    [
        sql::CLEANUP_GROUPS_FIRST,
        sql::CLEANUP_GROUPS_AFTER,
        sql::CLEANUP_SEARCH_FIRST,
        sql::CLEANUP_SEARCH_AFTER,
        sql::CLEANUP_METADATA_FIRST,
        sql::CLEANUP_METADATA_AFTER,
        sql::CLEANUP_SCAN_FIRST,
        sql::CLEANUP_SCAN_AFTER,
        sql::DISCOVERY_SCAN_FIRST,
        sql::DISCOVERY_SCAN_AFTER,
        sql::DISCOVERY_UPSERT_FIRST,
        sql::DISCOVERY_UPSERT_AFTER,
        sql::BUILD_SCAN_FIRST,
        sql::BUILD_SCAN_AFTER,
        sql::BUILD_INSERT_SEARCH_FIRST,
        sql::BUILD_INSERT_SEARCH_AFTER,
        sql::BUILD_INTERVAL_COUNT,
        sql::BUILD_INSERT_METADATA,
    ]
}

fn capture_grouped_sql(sql: &str) {
    GROUPED_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .push(sql.to_owned());
}

fn explain(connection: &Connection, sql: &str, values: Vec<Value>) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("query plan should prepare");
    statement
        .query_map(params_from_iter(values), |row| row.get(3))
        .expect("query plan should execute")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("query plan should collect")
}

fn assert_keyset_plan(details: &[String], alias: &str, range: &str) {
    let joined = details.join("\n");
    assert!(
        details.iter().any(|detail| {
            detail.contains(&format!("SEARCH {alias}")) && detail.contains(range)
        }),
        "expected an indexed {alias} keyset containing '{range}', got:\n{joined}"
    );
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
    let transaction = connection.transaction().expect("transaction should start");
    operation(&transaction);
    transaction
        .rollback()
        .expect("measurement should roll back");
    connection.progress_handler(0, None::<fn() -> bool>);
    steps.load(Ordering::Relaxed)
}

fn continuation_progress() -> Progress {
    Progress {
        projection_version: 2,
        stage: CodeReferenceSearchRebuildStage::Discover,
        completed_page_ordinal: 1,
        cleanup_cursor_rowid: None,
        cleanup_cursor_record_id: None,
        discovery_cursor_reference_id: Some("reference-0511".to_owned()),
        build_cursor_group_id: None,
        expected_reference_count: 2_048,
        cleanup_total_count: 0,
        discovered_reference_count: 512,
        discovered_group_count: 128,
        build_total_count: 0,
        cleaned_count: 0,
        built_count: 0,
        page_document_limit: 512,
        page_byte_limit: 1_048_576,
    }
}

fn grouped_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_files (
                 source_scope TEXT NOT NULL, path TEXT NOT NULL, language_id TEXT NOT NULL,
                 PRIMARY KEY (source_scope, path)
             );
             CREATE TABLE code_repository_references (
                 source_scope TEXT NOT NULL, reference_id TEXT NOT NULL,
                 name TEXT NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL, target_hint TEXT,
                 PRIMARY KEY (source_scope, reference_id)
             );
             CREATE TABLE code_repository_reference_search_groups (
                 source_scope TEXT NOT NULL, group_id TEXT NOT NULL,
                 name TEXT NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL,
                 target_hint TEXT NOT NULL, language_id TEXT NOT NULL,
                 occurrence_count INTEGER NOT NULL,
                 PRIMARY KEY (source_scope, group_id),
                 UNIQUE (source_scope, name, kind, path, target_hint)
             );
             CREATE VIRTUAL TABLE code_repository_search USING fts5(
                 source_scope UNINDEXED, document_kind UNINDEXED, record_id UNINDEXED,
                 path UNINDEXED, language_id UNINDEXED, content
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL, document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL, path TEXT NOT NULL,
                 search_rowid INTEGER PRIMARY KEY,
                 UNIQUE (source_scope, document_kind, record_id)
             );
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
                 'scope', 'repo', 'finalizing:rebuild_reference_search', 'commit', 'tree',
                 '[]', '[]', 1, 1, 1, 1, 2048, 0, 1, NULL,
                 '{\"max_files_per_batch\":1,\"max_bytes_per_batch\":1048576,\"max_rows_per_batch\":1538}',
                 1, NULL
             );",
        )
        .expect("grouped schema should initialize");
    connection
}

fn seed_discovery_fixture(connection: &mut Connection) {
    let transaction = connection
        .transaction()
        .expect("seed transaction should start");
    for path_index in 0..8 {
        transaction
            .execute(
                "INSERT INTO code_repository_files (source_scope, path, language_id)
                 VALUES ('scope', ?1, 'rust')",
                [format!("src/path-{path_index:02}.rs")],
            )
            .expect("file should insert");
    }
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO code_repository_references
                 (source_scope, reference_id, name, kind, path, target_hint)
                 VALUES ('scope', ?1, ?2, 'call', ?3, ?2)",
            )
            .expect("reference insert should prepare");
        for index in 0..2_048 {
            let identity = index % 128;
            statement
                .execute(params![
                    format!("reference-{index:04}"),
                    format!("Target{identity:03}"),
                    format!("src/path-{:02}.rs", identity % 8),
                ])
                .expect("reference should insert");
        }
    }
    for identity in 0..128 {
        transaction
            .execute(
                "INSERT INTO code_repository_reference_search_groups
                 (source_scope, group_id, name, kind, path, target_hint,
                  language_id, occurrence_count)
                 VALUES ('scope', ?1, ?2, 'call', ?3, ?2, 'rust', 4)",
                params![
                    format!("reference-{identity:04}"),
                    format!("Target{identity:03}"),
                    format!("src/path-{:02}.rs", identity % 8),
                ],
            )
            .expect("first-page group should insert");
    }
    transaction
        .commit()
        .expect("seed transaction should commit");
}
