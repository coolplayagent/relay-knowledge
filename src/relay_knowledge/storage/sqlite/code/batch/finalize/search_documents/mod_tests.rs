//! Direct tests for finalized search-document replacement and metadata synchronization.

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, params};

use super::{
    ReferenceSearchAdvance, advance_reference_search_progress,
    initialize_reference_search_progress as initialize_reference_search_owner,
};
use crate::domain::{
    CodeIndexResourceBudget, CodeReferenceSearchRebuild, CodeReferenceSearchRebuildStage,
};

#[test]
fn code_index_task_grouped_reference_search_rebuilds_bounded_pages_with_exact_counts() {
    let mut connection = search_database();
    seed_reference_page_fixture(&connection);
    let budget = CodeIndexResourceBudget::new(2, 1024 * 1024, 8).expect("budget should build");

    let initialized = {
        let transaction = connection.transaction().expect("transaction should open");
        let advance = initialize_reference_search_progress(&transaction, "scope", budget, 5)
            .expect("progress should initialize");
        transaction
            .commit()
            .expect("initial progress should commit");
        advance
    };
    assert_eq!(
        initialized,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        }
    );

    let mut checkpoint = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Cleanup,
        completed_page_ordinal: 0,
    };
    let mut observed_build_pages = Vec::new();
    loop {
        let transaction = connection.transaction().expect("transaction should open");
        let advance = advance_reference_search_progress(&transaction, "scope", checkpoint)
            .expect("one page should advance");
        transaction.commit().expect("page should commit");
        match advance {
            ReferenceSearchAdvance::Pending {
                stage,
                completed_page_ordinal,
            } => {
                checkpoint = CodeReferenceSearchRebuild {
                    protocol_version: 2,
                    stage,
                    completed_page_ordinal,
                };
                if stage == CodeReferenceSearchRebuildStage::Build {
                    observed_build_pages.push(completed_page_ordinal);
                }
            }
            ReferenceSearchAdvance::Complete => break,
        }
    }

    assert_eq!(observed_build_pages, [0, 1, 2]);
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        4
    );
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search_metadata"),
        4
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search
                 WHERE code_repository_search MATCH 'needle'
                   AND source_scope = 'scope' AND document_kind = 'reference'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("FTS match should count"),
        1
    );
    for (term, expected_count) in [
        ("firsthttpclient", 1),
        ("crate", 1),
        ("net", 1),
        ("httpclient", 1),
        ("needle", 1),
        ("src", 4),
        ("one", 1),
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_search
                     WHERE code_repository_search MATCH ?1
                       AND source_scope = 'scope' AND document_kind = 'reference'",
                    params![term],
                    |row| row.get::<_, usize>(0),
                )
                .expect("staged canonical term should count"),
            expected_count,
            "missing staged grouped reference term {term}"
        );
    }
    let whitespace_content = connection
        .query_row(
            "SELECT content FROM code_repository_search
             WHERE source_scope = 'scope' AND record_id = 'reference:03'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("whitespace target row should load");
    assert_eq!(whitespace_content, "third read src/three.rs");
    assert_eq!(
        connection
            .query_row(
                "SELECT projection_version, reference_count, group_count
                 FROM code_repository_reference_search_manifests WHERE source_scope = 'scope'",
                [],
                |row| Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?
                )),
            )
            .expect("grouped manifest should load"),
        (2, 5, 4)
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT occurrence_count FROM code_repository_reference_search_groups
                 WHERE source_scope = 'scope' AND group_id = 'reference:01'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("dense group should load"),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search
                 WHERE source_scope = 'scope' AND document_kind = 'symbol'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("other kinds should count"),
        1
    );
}

#[test]
fn code_index_task_grouped_reference_search_crash_resume_replays_exact_page() {
    let database_path = temporary_search_database_path();
    let mut connection = initialize_search_database(
        Connection::open(&database_path).expect("file database should open"),
    );
    seed_reference_page_fixture(&connection);
    let budget = CodeIndexResourceBudget::new(2, 1024 * 1024, 8).expect("budget should build");
    let transaction = connection.transaction().expect("transaction should open");
    initialize_reference_search_progress(&transaction, "scope", budget, 5)
        .expect("progress should initialize");
    transaction.commit().expect("progress should commit");
    let cleanup = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Cleanup,
        completed_page_ordinal: 0,
    };
    let transaction = connection.transaction().expect("transaction should open");
    let rolled_back = advance_reference_search_progress(&transaction, "scope", cleanup)
        .expect("cleanup page should execute");
    assert_eq!(
        rolled_back,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 1,
        }
    );
    transaction.rollback().expect("page should roll back");

    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        2
    );
    let transaction = connection.transaction().expect("transaction should reopen");
    let replayed = advance_reference_search_progress(&transaction, "scope", cleanup)
        .expect("rolled back page should replay");
    transaction.commit().expect("replayed page should commit");
    assert_eq!(replayed, rolled_back);
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        0
    );

    let cleanup_page_one = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Cleanup,
        completed_page_ordinal: 1,
    };
    let transaction = connection.transaction().expect("transaction should open");
    let discover_start = advance_reference_search_progress(&transaction, "scope", cleanup_page_one)
        .expect("cleanup should transition to discovery");
    transaction
        .commit()
        .expect("discovery transition should commit");
    assert_eq!(
        discover_start,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Discover,
            completed_page_ordinal: 0,
        }
    );
    let discover = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Discover,
        completed_page_ordinal: 0,
    };
    let transaction = connection.transaction().expect("transaction should open");
    let rolled_back_discovery = advance_reference_search_progress(&transaction, "scope", discover)
        .expect("discovery page should execute");
    transaction
        .rollback()
        .expect("discovery page should roll back");
    drop(connection);

    let mut connection = Connection::open(&database_path).expect("database should reopen");
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        0
    );
    let transaction = connection.transaction().expect("transaction should reopen");
    let replayed_discovery = advance_reference_search_progress(&transaction, "scope", discover)
        .expect("rolled back discovery page should replay after reopen");
    transaction
        .commit()
        .expect("replayed discovery page should commit");
    assert_eq!(replayed_discovery, rolled_back_discovery);
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        0
    );
    let mut checkpoint = CodeReferenceSearchRebuild {
        protocol_version: 2,
        stage: CodeReferenceSearchRebuildStage::Discover,
        completed_page_ordinal: 1,
    };
    loop {
        let transaction = connection.transaction().expect("transaction should open");
        let advance = advance_reference_search_progress(&transaction, "scope", checkpoint)
            .expect("discovery should advance");
        transaction.commit().expect("discovery should commit");
        let ReferenceSearchAdvance::Pending {
            stage,
            completed_page_ordinal,
        } = advance
        else {
            panic!("discovery cannot complete the whole projection");
        };
        checkpoint = CodeReferenceSearchRebuild {
            protocol_version: 2,
            stage,
            completed_page_ordinal,
        };
        if stage == CodeReferenceSearchRebuildStage::Build {
            break;
        }
    }
    let transaction = connection.transaction().expect("transaction should open");
    let rolled_back_build = advance_reference_search_progress(&transaction, "scope", checkpoint)
        .expect("build page should execute");
    transaction.rollback().expect("build page should roll back");
    drop(connection);

    let mut connection = Connection::open(&database_path).expect("database should reopen");
    let transaction = connection.transaction().expect("transaction should reopen");
    let replayed_build = advance_reference_search_progress(&transaction, "scope", checkpoint)
        .expect("rolled back build page should replay after reopen");
    transaction
        .commit()
        .expect("replayed build page should commit");
    assert_eq!(replayed_build, rolled_back_build);
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        2
    );
    drop(connection);
    std::fs::remove_file(database_path).expect("temporary database should be removed");
}

#[test]
fn staged_reference_search_rejects_pages_outside_row_and_byte_budgets() {
    let mut connection = search_database();
    seed_reference_page_fixture(&connection);
    let four_row_budget =
        CodeIndexResourceBudget::new(2, 1024 * 1024, 4).expect("budget should build");
    let transaction = connection.transaction().expect("transaction should open");
    let row_error = initialize_reference_search_progress(&transaction, "scope", four_row_budget, 5)
        .expect_err("four rows cannot cover three owners and two control mutations");
    assert!(matches!(
        row_error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    transaction
        .rollback()
        .expect("row-bound attempt should roll back");

    let five_row_budget =
        CodeIndexResourceBudget::new(2, 1024 * 1024, 5).expect("budget should build");
    let transaction = connection.transaction().expect("transaction should open");
    initialize_reference_search_progress(&transaction, "scope", five_row_budget, 5)
        .expect("five rows should admit one document plus two control mutations");
    assert_eq!(
        transaction
            .query_row(
                "SELECT page_document_limit
                 FROM code_repository_reference_search_progress WHERE source_scope = 'scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("page limit should load"),
        1
    );
    transaction
        .rollback()
        .expect("five-row boundary probe should roll back");

    let one_byte_budget = CodeIndexResourceBudget::new(2, 1, 5).expect("budget should build");
    let transaction = connection.transaction().expect("transaction should open");
    let byte_error =
        initialize_reference_search_progress(&transaction, "scope", one_byte_budget, 5)
            .expect_err("the progress and checkpoint control records exceed one byte");
    assert!(matches!(
        byte_error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    transaction
        .rollback()
        .expect("byte-bound initialization should roll back");
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        2
    );
}

#[test]
fn code_index_task_legacy_reference_search_restart_clamps_v1_page_limit_to_v2_budget() {
    let mut connection = search_database();
    seed_reference_page_fixture(&connection);
    let budget = CodeIndexResourceBudget::new(2, 1024 * 1024, 16).expect("budget should build");
    connection
        .execute(
            "UPDATE code_repository_index_checkpoints
             SET committed_reference_count = 5, resource_budget_json = ?1
             WHERE source_scope = 'scope'",
            params![serde_json::to_string(&budget).expect("budget should encode")],
        )
        .expect("checkpoint budget should insert");
    connection
        .execute(
            "INSERT INTO code_repository_reference_search_progress (
                 source_scope, projection_version, stage, completed_page_ordinal,
                 cleanup_cursor_rowid, cleanup_cursor_record_id,
                 discovery_cursor_reference_id, build_cursor_group_id,
                 expected_reference_count, cleanup_total_count,
                 discovered_reference_count, discovered_group_count,
                 build_total_count, cleaned_count, built_count,
                 page_document_limit, page_byte_limit
             ) VALUES (
                 'scope', 1, 'build', 7, NULL, NULL, NULL, 'reference:05',
                 5, 2, 0, 0, 5, 2, 5, 8, 1048576
             )",
            [],
        )
        .expect("legacy v1 progress should insert");

    let transaction = connection.transaction().expect("transaction should open");
    let restarted = advance_reference_search_progress(
        &transaction,
        "scope",
        CodeReferenceSearchRebuild {
            protocol_version: 1,
            stage: CodeReferenceSearchRebuildStage::Build,
            completed_page_ordinal: 7,
        },
    )
    .expect("leased legacy progress should restart atomically");
    transaction.commit().expect("restart should commit");
    assert_eq!(
        restarted,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        }
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT projection_version, page_document_limit, page_byte_limit
                 FROM code_repository_reference_search_progress WHERE source_scope = 'scope'",
                [],
                |row| {
                    Ok((
                        row.get::<_, usize>(0)?,
                        row.get::<_, usize>(1)?,
                        row.get::<_, usize>(2)?,
                    ))
                },
            )
            .expect("restarted limits should load"),
        (2, 4, 1024 * 1024)
    );
    let transaction = connection.transaction().expect("transaction should open");
    let cleanup = advance_reference_search_progress(
        &transaction,
        "scope",
        CodeReferenceSearchRebuild {
            protocol_version: 2,
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        },
    )
    .expect("v2 cleanup should use the clamped page");
    transaction.commit().expect("cleanup should commit");
    assert_eq!(
        cleanup,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 1,
        }
    );
}

#[test]
fn staged_reference_search_rejects_metadata_without_an_exact_fts_owner() {
    let mut connection = search_database();
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata (
                 source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('scope', 'reference', 'orphan:metadata', 'src/orphan.rs', 999999)",
            [],
        )
        .expect("orphan metadata fixture should insert");
    let budget = CodeIndexResourceBudget::new(2, 1024 * 1024, 5).expect("budget should build");

    let transaction = connection.transaction().expect("transaction should open");
    initialize_reference_search_progress(&transaction, "scope", budget, 0)
        .expect("constant-time progress initialization should not scan the scope");
    transaction.commit().expect("progress should commit");
    let transaction = connection.transaction().expect("transaction should open");
    let error = advance_reference_search_progress(
        &transaction,
        "scope",
        CodeReferenceSearchRebuild {
            protocol_version: 2,
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        },
    )
    .expect_err("the bounded cleanup owner check must reject invalid metadata");
    assert!(matches!(
        error,
        crate::storage::StorageError::Invariant(message)
            if message.contains("reference-search cleanup")
    ));
    transaction
        .rollback()
        .expect("failed page should roll back");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_reference_search_progress
                 WHERE source_scope = 'scope' AND stage = 'cleanup'
                   AND completed_page_ordinal = 0 AND cleanup_total_count = 0
                   AND cleaned_count = 0",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("unchanged progress should count"),
        1
    );
}

#[test]
fn staged_reference_search_rejects_cleanup_owners_exceeding_frozen_facts() {
    let mut connection = search_database();
    seed_reference_page_fixture(&connection);
    connection
        .execute(
            "DELETE FROM code_repository_references
             WHERE source_scope = 'scope' AND reference_id <> 'reference:01'",
            [],
        )
        .expect("deleted facts should model a replacement with fewer references");
    let budget = CodeIndexResourceBudget::new(2, 1024 * 1024, 8).expect("budget should build");

    let transaction = connection.transaction().expect("transaction should open");
    let advance = initialize_reference_search_progress(&transaction, "scope", budget, 1)
        .expect("constant-time progress should initialize from the frozen fact count");
    transaction.commit().expect("progress should commit");
    assert_eq!(
        advance,
        ReferenceSearchAdvance::Pending {
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        }
    );

    let transaction = connection.transaction().expect("transaction should open");
    let error = advance_reference_search_progress(
        &transaction,
        "scope",
        CodeReferenceSearchRebuild {
            protocol_version: 2,
            stage: CodeReferenceSearchRebuildStage::Cleanup,
            completed_page_ordinal: 0,
        },
    )
    .expect_err("a staged scope cannot own more old reference documents than frozen facts");
    assert!(matches!(
        error,
        crate::storage::StorageError::Invariant(message)
            if message.contains("cleanup counts")
    ));
    transaction
        .rollback()
        .expect("failed page should roll back");
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search"),
        2
    );
    assert_eq!(
        count_reference_rows(&connection, "code_repository_search_metadata"),
        2
    );
}

fn seed_reference_page_fixture(connection: &Connection) {
    for (reference_id, path, name, kind, target_hint) in [
        (
            "reference:01",
            "src/one.rs",
            "firstHttpClient",
            "read",
            Some("crate::net::HttpClient::needle"),
        ),
        (
            "reference:02",
            "src/one.rs",
            "firstHttpClient",
            "read",
            Some("crate::net::HttpClient::needle"),
        ),
        ("reference:03", "src/three.rs", "third", "read", Some("   ")),
        (
            "reference:04",
            "src/four.rs",
            "fourth",
            "call",
            Some("Target"),
        ),
        ("reference:05", "src/five.rs", "fifth", "read", Some("Hint")),
    ] {
        connection
            .execute(
                "INSERT OR IGNORE INTO code_repository_files (source_scope, path, language_id)
                 VALUES ('scope', ?1, 'rust')",
                params![path],
            )
            .expect("file should insert");
        connection
            .execute(
                "INSERT INTO code_repository_references (
                     source_scope, reference_id, path, name, kind, target_hint
                 ) VALUES ('scope', ?1, ?2, ?3, ?4, ?5)",
                params![reference_id, path, name, kind, target_hint],
            )
            .expect("reference should insert");
    }
    for stale_id in ["stale:1", "stale:2"] {
        connection
            .execute(
                "INSERT INTO code_repository_search (
                     source_scope, document_kind, record_id, path, language_id, content
                 ) VALUES ('scope', 'reference', ?1, 'src/stale.rs', 'rust', 'stale')",
                params![stale_id],
            )
            .expect("stale FTS row should insert");
        let search_rowid = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO code_repository_search_metadata (
                     source_scope, document_kind, record_id, path, search_rowid
                 ) VALUES ('scope', 'reference', ?1, 'src/stale.rs', ?2)",
                params![stale_id, search_rowid],
            )
            .expect("stale metadata should insert");
    }
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 source_scope, document_kind, record_id, path, language_id, content
             ) VALUES ('scope', 'symbol', 'symbol:1', 'src/one.rs', 'rust', 'symbol')",
            [],
        )
        .expect("other-kind FTS row should insert");
    let symbol_rowid = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata (
                 source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('scope', 'symbol', 'symbol:1', 'src/one.rs', ?1)",
            params![symbol_rowid],
        )
        .expect("other-kind metadata should insert");
}

fn count_reference_rows(connection: &Connection, table: &str) -> usize {
    let query = match table {
        "code_repository_search" => {
            "SELECT COUNT(*) FROM code_repository_search
             WHERE source_scope = 'scope' AND document_kind = 'reference'"
        }
        "code_repository_search_metadata" => {
            "SELECT COUNT(*) FROM code_repository_search_metadata
             WHERE source_scope = 'scope' AND document_kind = 'reference'"
        }
        _ => panic!("unsupported test table"),
    };
    connection
        .query_row(query, [], |row| row.get(0))
        .expect("reference rows should count")
}

fn initialize_reference_search_progress(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<ReferenceSearchAdvance, crate::storage::StorageError> {
    transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET committed_reference_count = ?2, resource_budget_json = ?3
         WHERE source_scope = ?1",
        params![
            source_scope,
            expected_reference_count,
            serde_json::to_string(&resource_budget).expect("test budget should encode"),
        ],
    )?;
    initialize_reference_search_owner(
        transaction,
        source_scope,
        resource_budget,
        expected_reference_count,
    )
}

pub(super) fn search_database() -> Connection {
    initialize_search_database(Connection::open_in_memory().expect("database should open"))
}

fn initialize_search_database(connection: Connection) -> Connection {
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                PRIMARY KEY (source_scope, path)
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT NOT NULL,
                reference_id TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_hint TEXT,
                PRIMARY KEY (source_scope, reference_id)
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            CREATE TABLE code_repository_search_metadata (
                source_scope TEXT NOT NULL,
                document_kind TEXT NOT NULL,
                record_id TEXT NOT NULL,
                path TEXT NOT NULL,
                search_rowid INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (source_scope, document_kind, record_id)
            );
            CREATE TABLE code_repository_reference_search_progress (
                source_scope TEXT PRIMARY KEY,
                projection_version INTEGER NOT NULL,
                stage TEXT NOT NULL,
                completed_page_ordinal INTEGER NOT NULL,
                cleanup_cursor_rowid INTEGER,
                cleanup_cursor_record_id TEXT,
                discovery_cursor_reference_id TEXT,
                build_cursor_group_id TEXT,
                expected_reference_count INTEGER NOT NULL,
                cleanup_total_count INTEGER NOT NULL,
                discovered_reference_count INTEGER NOT NULL,
                discovered_group_count INTEGER NOT NULL,
                build_total_count INTEGER NOT NULL,
                cleaned_count INTEGER NOT NULL,
                built_count INTEGER NOT NULL,
                page_document_limit INTEGER NOT NULL,
                page_byte_limit INTEGER NOT NULL
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
                '[]', '[]', 1, 1, 1, 1, 0, 0, 1, NULL,
                '{\"max_files_per_batch\":1,\"max_bytes_per_batch\":1048576,\"max_rows_per_batch\":5}',
                1, NULL
            );
            CREATE TABLE code_repository_reference_search_groups (
                source_scope TEXT NOT NULL,
                group_id TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                path TEXT NOT NULL,
                target_hint TEXT NOT NULL,
                language_id TEXT NOT NULL,
                occurrence_count INTEGER NOT NULL,
                PRIMARY KEY (source_scope, group_id),
                UNIQUE (source_scope, name, kind, path, target_hint)
            );
            CREATE TABLE code_repository_reference_search_manifests (
                source_scope TEXT PRIMARY KEY,
                projection_version INTEGER NOT NULL,
                reference_count INTEGER NOT NULL,
                group_count INTEGER NOT NULL
            );
            ",
        )
        .expect("search schema should be created");
    connection
}

fn temporary_search_database_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-reference-search-{}-{nonce}.sqlite",
        std::process::id()
    ))
}
