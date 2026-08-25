use super::*;

#[test]
fn creates_checkpoint_and_claimable_task_schema() {
    let connection = Connection::open_in_memory().expect("database should open");

    initialize_index_task_schema(&connection).expect("index task schema should initialize");
    initialize_index_task_schema(&connection).expect("index task schema should be idempotent");

    let table_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'table'
              AND name IN (
                  'code_repository_index_checkpoints',
                  'code_repository_index_tasks',
                  'code_repository_index_batch_staging',
                  'code_repository_reference_search_groups',
                  'code_repository_reference_search_manifests',
                  'code_repository_reference_search_progress',
                  'code_repository_reference_resolution_progress',
                  'code_repository_incremental_clone_progress',
                  'code_repository_publication_fences',
                  'code_repository_publication_receipts'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("index task tables should be inspectable");
    assert_eq!(table_count, 10);

    let generation_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_index_tasks') WHERE name = 'publication_generation'",
            [],
            |row| row.get(0),
        )
        .expect("publication generation should be inspectable");
    assert_eq!(generation_column_count, 1);

    let index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name IN (
                  'code_repository_index_tasks_claimable',
                  'code_repository_index_tasks_repository',
                  'code_repository_index_tasks_repository_fifo',
                  'code_repository_index_tasks_audit_retention',
                  'code_repository_index_tasks_publication_retention',
                  'code_repository_index_tasks_scope_retention',
                  'code_repository_index_checkpoints_repository_scope',
                  'code_repository_index_checkpoints_publication_retention',
                  'code_repository_index_batch_staging_state',
                  'code_repository_reference_search_groups_path',
                  'code_repository_incremental_clone_progress_task'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("task indexes should be inspectable");
    assert_eq!(index_count, 11);

    let fifo_columns = connection
        .prepare("PRAGMA index_info(code_repository_index_tasks_repository_fifo)")
        .expect("repository FIFO index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("repository FIFO index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("repository FIFO index columns should collect");
    assert_eq!(fifo_columns, ["repository_id", "created_at_ms", "task_id"]);

    let checkpoint_columns = connection
        .prepare("PRAGMA index_info(code_repository_index_checkpoints_repository_scope)")
        .expect("checkpoint retention index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("checkpoint retention index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("checkpoint retention index columns should collect");
    assert_eq!(checkpoint_columns, ["repository_id", "source_scope"]);

    let checkpoint_publication_columns = connection
        .prepare("PRAGMA index_info(code_repository_index_checkpoints_publication_retention)")
        .expect("checkpoint publication index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("checkpoint publication index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("checkpoint publication index columns should collect");
    assert_eq!(
        checkpoint_publication_columns,
        ["repository_id", "state", "updated_at_ms", "source_scope"]
    );

    let columns = connection
        .prepare("PRAGMA index_info(code_repository_index_tasks_scope_retention)")
        .expect("retention index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("retention index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("retention index columns should collect");
    assert_eq!(
        columns,
        ["source_scope", "state", "updated_at_ms", "task_id"]
    );

    let publication_columns = connection
        .prepare("PRAGMA index_info(code_repository_index_tasks_publication_retention)")
        .expect("publication retention index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("publication retention index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("publication retention columns should collect");
    assert_eq!(
        publication_columns,
        [
            "repository_id",
            "state",
            "publication_generation",
            "updated_at_ms",
            "created_at_ms",
            "task_id",
            "source_scope"
        ]
    );
}

#[test]
fn upgrades_legacy_task_table_before_creating_publication_index() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repositories (repository_id TEXT PRIMARY KEY);
            CREATE TABLE code_repository_index_tasks (
                task_id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                alias TEXT NOT NULL,
                ref_selector TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                tree_hash TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                mode_json TEXT NOT NULL,
                state TEXT NOT NULL,
                lease_owner TEXT,
                lease_expires_at_ms INTEGER,
                attempt_count INTEGER NOT NULL,
                next_retry_at_ms INTEGER NOT NULL,
                input_fingerprint TEXT NOT NULL,
                resource_budget_json TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                last_error_kind TEXT,
                last_error_message TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                UNIQUE (repository_id, input_fingerprint)
            );
            ",
        )
        .expect("legacy task table should initialize");

    initialize_index_task_schema(&connection).expect("legacy task schema should upgrade");

    let generation_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_index_tasks')
             WHERE name = 'publication_generation'",
            [],
            |row| row.get(0),
        )
        .expect("publication generation should be inspectable");
    let publication_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index'
               AND name = 'code_repository_index_tasks_publication_retention'",
            [],
            |row| row.get(0),
        )
        .expect("publication index should be inspectable");

    assert_eq!(generation_column_count, 1);
    assert_eq!(publication_index_count, 1);
}

#[test]
fn incremental_clone_schema_is_exact_across_a_file_reopen() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-incremental-clone-schema-{}-{nonce}.sqlite",
        std::process::id()
    ));
    for _ in 0..2 {
        let connection = Connection::open(&path).expect("database should open");
        initialize_index_task_schema(&connection).expect("clone schema should initialize");
        assert!(
            crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(
                &connection
            )
            .expect("clone marker should inspect")
        );
    }
    std::fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn incremental_clone_marker_repairs_a_checkpoint_missing_the_fact_proof_column() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_index_task_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(
            "ALTER TABLE code_repository_index_checkpoints
             DROP COLUMN committed_fact_row_count;",
        )
        .expect("legacy checkpoint shape should install");
    assert!(
        !crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(&connection)
            .expect("clone marker should inspect")
    );

    initialize_index_task_schema(&connection).expect("missing proof column should repair");
    let proof_default = connection
        .query_row(
            "SELECT dflt_value
             FROM pragma_table_xinfo('code_repository_index_checkpoints')
             WHERE name = 'committed_fact_row_count'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("fact proof column should exist");
    assert_eq!(proof_default, "0");
    assert!(
        crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(&connection)
            .expect("repaired clone marker should inspect")
    );
}

#[test]
fn active_incremental_clone_reopens_after_checkpoint_receipt_column_upgrade() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_index_task_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO code_repository_incremental_clone_progress (
                 source_scope, repository_id, base_scope, task_id, delta_digest,
                 protocol_version, phase, table_ordinal, completed_page_ordinal,
                 cursor_key, cursor_tiebreaker, completed_table_ordinal, expected_table_rows,
                 scanned_table_rows, copied_table_rows, scanned_total_rows,
                 copied_total_rows, copied_total_bytes, cloned_file_count,
                 cloned_symbol_count, cloned_reference_count, cloned_chunk_count,
                 cloned_diagnostic_count, cloned_reference_group_count,
                 cloned_search_document_count, base_manifest_reference_count,
                 base_manifest_group_count, scanned_reference_occurrence_count,
                 scanned_reference_row_count, scanned_reference_group_count,
                 scanned_reference_search_owner_count, base_source_fact_row_upper_bound,
                 page_row_limit, page_byte_limit, updated_at_ms
             ) VALUES (
                 'target', 'repo', 'base', 'task', 'digest',
                 1, 'tables', 0, 0, NULL, NULL, NULL, NULL,
                 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                 0, 0, 0, 0, 1, 1, 1, 1
             );
             ALTER TABLE code_repository_index_checkpoints
                 DROP COLUMN incremental_summary_json;
             PRAGMA foreign_keys = ON;",
        )
        .expect("legacy active clone shape should install");

    initialize_index_task_schema(&connection)
        .expect("checkpoint capability should upgrade before clone marker validation");

    let progress_rows = connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_incremental_clone_progress",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("progress owner should remain readable");
    assert_eq!(progress_rows, 1);
    assert!(
        crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(&connection)
            .expect("upgraded clone marker should inspect")
    );
}

#[test]
fn checkpoint_fact_proof_rejects_an_added_check_after_reopen() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-checkpoint-proof-check-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let connection = Connection::open(&path).expect("database should open");
        initialize_index_task_schema(&connection).expect("schema should initialize");
        let definition = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'table' AND name = 'code_repository_index_checkpoints'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("checkpoint definition should load");
        let malformed = definition.replacen(
            "committed_fact_row_count INTEGER NOT NULL DEFAULT 0",
            "committed_fact_row_count INTEGER NOT NULL DEFAULT 0
                 CHECK (committed_fact_row_count = 0)",
            1,
        );
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF; DROP TABLE code_repository_index_checkpoints;",
            )
            .expect("empty checkpoint should drop");
        connection
            .execute_batch(&malformed)
            .expect("checkpoint with an added proof check should create");
        connection
            .execute_batch(
                "CREATE INDEX code_repository_index_checkpoints_repository_scope
                     ON code_repository_index_checkpoints(repository_id, source_scope);
                 CREATE INDEX code_repository_index_checkpoints_publication_retention
                     ON code_repository_index_checkpoints(
                         repository_id, state, updated_at_ms DESC, source_scope DESC
                     );
                 PRAGMA foreign_keys = ON;",
            )
            .expect("canonical checkpoint indexes should restore");
    }
    {
        let connection = Connection::open(&path).expect("database should reopen");
        assert!(
            !crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(
                &connection
            )
            .expect("clone marker should inspect")
        );
        let error = initialize_index_task_schema(&connection)
            .expect_err("an added fact-proof check must fail closed");
        assert!(
            error
                .to_string()
                .contains("incremental-clone progress schema")
        );
    }
    std::fs::remove_file(path).expect("temporary database should be removable");
}

#[test]
fn checkpoint_fact_proof_rejects_extra_write_objects() {
    for hostile_object in [
        "CREATE INDEX hostile_checkpoint_fact_index
             ON code_repository_index_checkpoints(committed_fact_row_count)",
        "CREATE TRIGGER hostile_checkpoint_fact_trigger
             AFTER UPDATE OF committed_fact_row_count ON code_repository_index_checkpoints
             BEGIN SELECT 1; END",
    ] {
        let connection = Connection::open_in_memory().expect("database should open");
        initialize_index_task_schema(&connection).expect("schema should initialize");
        connection
            .execute_batch(hostile_object)
            .expect("hostile write object should install");

        assert!(
            !crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(
                &connection
            )
            .expect("clone marker should inspect")
        );
    }
}

#[test]
fn empty_noncanonical_incremental_clone_owner_is_repaired_atomically() {
    for affected_definition in [
        "CREATE TABLE code_repository_incremental_clone_affected_paths (
             source_scope TEXT NOT NULL, path TEXT NOT NULL DEFAULT '',
             PRIMARY KEY (source_scope, path),
             FOREIGN KEY (source_scope)
                 REFERENCES code_repository_incremental_clone_progress(source_scope)
                 ON DELETE CASCADE
         )",
        "CREATE TABLE code_repository_incremental_clone_affected_paths (
             source_scope TEXT NOT NULL, path TEXT NOT NULL CHECK (path <> ''),
             PRIMARY KEY (source_scope, path),
             FOREIGN KEY (source_scope)
                 REFERENCES code_repository_incremental_clone_progress(source_scope)
                 ON DELETE CASCADE
         )",
        "CREATE TABLE code_repository_incremental_clone_affected_paths (
             source_scope TEXT NOT NULL, path TEXT NOT NULL,
             PRIMARY KEY (source_scope, path),
             FOREIGN KEY (source_scope)
                 REFERENCES code_repository_incremental_clone_progress(source_scope)
                 ON DELETE CASCADE
         ) WITHOUT ROWID",
    ] {
        let connection = Connection::open_in_memory().expect("database should open");
        initialize_index_task_schema(&connection).expect("schema should initialize");
        connection
            .execute_batch("DROP TABLE code_repository_incremental_clone_affected_paths;")
            .expect("empty affected owner should drop");
        connection
            .execute_batch(affected_definition)
            .expect("malformed affected owner should create");

        initialize_index_task_schema(&connection).expect("empty owner should repair");
        assert!(
            crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(
                &connection
            )
            .expect("clone marker should inspect")
        );
    }
}

#[test]
fn nonempty_noncanonical_incremental_clone_owner_fails_closed() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_index_task_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             DROP TABLE code_repository_incremental_clone_affected_paths;
             CREATE TABLE code_repository_incremental_clone_affected_paths (
                 source_scope TEXT NOT NULL, path TEXT NOT NULL CHECK (path <> ''),
                 PRIMARY KEY (source_scope, path),
                 FOREIGN KEY (source_scope)
                     REFERENCES code_repository_incremental_clone_progress(source_scope)
                     ON DELETE CASCADE
             );
             INSERT INTO code_repository_incremental_clone_affected_paths
                 (source_scope, path) VALUES ('scope', 'src/lib.rs');
             PRAGMA foreign_keys = ON;",
        )
        .expect("malformed nonempty owner should create");

    let error = initialize_index_task_schema(&connection)
        .expect_err("nonempty malformed owner must fail closed");
    assert!(
        error
            .to_string()
            .contains("non-empty incremental-clone owner")
    );
}
