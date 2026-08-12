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
                  'code_repository_publication_fences'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("index task tables should be inspectable");
    assert_eq!(table_count, 4);

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
                  'code_repository_index_batch_staging_state'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("task indexes should be inspectable");
    assert_eq!(index_count, 9);

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
