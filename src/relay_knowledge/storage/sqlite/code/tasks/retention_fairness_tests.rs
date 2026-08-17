use crate::{
    domain::CodeRepositoryRegistration,
    storage::{CodeRepositoryStore, CodeScopeRetentionRequest, SqliteGraphStore, StorageError},
};

#[tokio::test]
async fn code_index_task_retention_fairly_advances_scope_task_and_alias_backlogs() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", vec![], vec![])
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
        .run(|connection| {
            connection.execute_batch(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES
                     ('scope-active', 'repo', 'commit-active', 'tree-active', '[]', '[]',
                      0, 0, 0, 0, 0, NULL),
                     ('scope-old', 'repo', 'commit-old', 'tree-old', '[]', '[]',
                      1200, 0, 0, 0, 0, NULL);
                 UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-active',
                     last_indexed_commit = 'commit-active', tree_hash = 'tree-active'
                 WHERE repository_id = 'repo';
                 WITH RECURSIVE sequence(value) AS (
                     SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value < 1199
                 )
                 INSERT INTO code_repository_files (
                     repository_id, source_scope, file_id, path, language_id, blob_hash,
                     byte_len, line_count, parse_status, degraded_reason
                 )
                 SELECT 'repo', 'scope-old', 'file-' || value, 'src/' || value || '.rs',
                        'rust', 'blob', 1, 1, 'parsed', NULL FROM sequence;",
            )?;
            Ok(())
        })
        .await
        .expect("scope backlog should insert");

    let scheduled = maintenance_pass(&store).await;
    assert_eq!(scheduled.retiring_job_count, 1);
    store
        .run(|connection| {
            insert_terminal_history(connection, "succeeded", "success", 700)?;
            insert_terminal_history(connection, "dead_letter", "failed", 700)?;
            connection.execute(
                "WITH RECURSIVE sequence(value) AS (
                     SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value < 799
                 )
                 INSERT INTO code_repository_commit_scopes (
                     repository_id, resolved_commit_sha, source_scope, published_sequence
                 )
                 SELECT 'repo', 'alias-' || value, 'scope-active', value + 1 FROM sequence",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("audit backlogs should insert");
    let before = backlog_counts(&store).await;

    let progressed = maintenance_pass(&store).await;
    let after = backlog_counts(&store).await;

    assert!(progressed.retiring_jobs[0].phase != scheduled.retiring_jobs[0].phase);
    assert_eq!(
        before.0 - after.0,
        super::RETAIN_SUCCEEDED_TASK_AUDIT_ROWS * 4
    );
    assert_eq!(before.1 - after.1, super::retention_gc::GC_ROW_BATCH_SIZE);
    assert_eq!(before.2 - after.2, super::retention_gc::GC_ROW_BATCH_SIZE);
    assert_eq!(after.3, 1_200, "the large scope still has physical GC work");
}

fn insert_terminal_history(
    connection: &mut rusqlite::Connection,
    state: &str,
    prefix: &str,
    count: usize,
) -> Result<(), StorageError> {
    let sql = format!(
        "WITH RECURSIVE sequence(value) AS (
             SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value < ?1
         )
         INSERT INTO code_repository_index_tasks (
             task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
             source_scope, path_filters_json, language_filters_json, mode_json, state,
             attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
             payload_json, created_at_ms, updated_at_ms
         )
         SELECT '{prefix}-' || value, 'repo', 'fixture', 'main', 'commit-' || value,
                'tree', 'scope-active', '[]', '[]', '\"full\"', ?2, 1, 0,
                '{prefix}-' || value, '{{}}', '{{}}', value, value
         FROM sequence"
    );
    connection.execute(&sql, rusqlite::params![count - 1, state])?;
    Ok(())
}

async fn maintenance_pass(store: &SqliteGraphStore) -> crate::domain::CodeScopeRetentionSummary {
    store
        .prune_code_repository_scopes(CodeScopeRetentionRequest {
            repository_id: "repo".to_owned(),
            active_scope: "scope-active".to_owned(),
            retain_recent_successful_scopes: 0,
            repository_retention_cutoff_ms: None,
            repository_retention_cutoff_generation: None,
            repository_retention_initial_scope: None,
        })
        .await
        .expect("maintenance pass should succeed")
}

async fn backlog_counts(store: &SqliteGraphStore) -> (usize, usize, usize, usize) {
    store
        .run(|connection| {
            Ok((
                count(
                    connection,
                    "code_repository_index_tasks",
                    "state = 'succeeded'",
                )?,
                count(
                    connection,
                    "code_repository_index_tasks",
                    "state = 'dead_letter'",
                )?,
                count(connection, "code_repository_commit_scopes", "1 = 1")?,
                count(
                    connection,
                    "code_repository_files",
                    "source_scope = 'scope-old'",
                )?,
            ))
        })
        .await
        .expect("backlog counts should query")
}

fn count(
    connection: &rusqlite::Connection,
    table: &'static str,
    predicate: &'static str,
) -> Result<usize, StorageError> {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
            [],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}
