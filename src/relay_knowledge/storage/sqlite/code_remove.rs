use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{domain::CodeRepositoryRemovalSummary, storage::StorageError};

use super::{code_cleanup::delete_scope_index, code_status};

pub(super) fn remove_repository(
    connection: &mut Connection,
    repository: &str,
    now_ms: u64,
) -> Result<Option<CodeRepositoryRemovalSummary>, StorageError> {
    let Some(status) = code_status::repository_status(connection, repository)? else {
        return Ok(None);
    };
    let repository_id = status.repository_id;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    reject_running_index_task(&transaction, &repository_id, now_ms)?;
    let aliases_removed = repository_aliases(&transaction, &repository_id)?;
    let scopes = repository_cleanup_scopes(&transaction, &repository_id)?;
    let affected_set_ids = affected_repository_sets(&transaction, &repository_id)?;
    reject_running_repository_set_refresh_tasks(&transaction, &affected_set_ids, now_ms)?;
    let removed_repository_set_member_count =
        count_repository_set_members(&transaction, &repository_id)?;
    let removed_index_task_count = count_index_tasks(&transaction, &repository_id)?;

    invalidate_repository_sets(&transaction, &affected_set_ids)?;
    transaction.execute(
        "DELETE FROM code_repository_set_members WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    for scope in &scopes {
        delete_scope_index(&transaction, scope)?;
        delete_scope_lifecycle_projection(&transaction, scope)?;
    }
    transaction.execute(
        "DELETE FROM code_repository_index_checkpoints WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_index_tasks WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_scopes WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_aliases WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repositories WHERE repository_id = ?1",
        params![&repository_id],
    )?;
    transaction.commit()?;

    Ok(Some(CodeRepositoryRemovalSummary {
        repository_id,
        aliases_removed,
        removed_scope_count: scopes.len(),
        removed_index_task_count,
        removed_repository_set_member_count,
        invalidated_repository_set_count: affected_set_ids.len(),
    }))
}

fn reject_running_index_task(
    transaction: &Transaction<'_>,
    repository_id: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    let running_task_id = transaction
        .query_row(
            "
            SELECT task_id
            FROM code_repository_index_tasks
            WHERE repository_id = ?1
              AND state = 'running'
              AND lease_expires_at_ms > ?2
            ORDER BY lease_expires_at_ms DESC, created_at_ms ASC, task_id ASC
            LIMIT 1
            ",
            params![repository_id, now_ms],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(task_id) = running_task_id {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has running index task '{task_id}'; wait for the task to finish before removing the repository"
        )));
    }

    Ok(())
}

fn repository_aliases(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT alias
        FROM code_repository_aliases
        WHERE repository_id = ?1
        UNION
        SELECT alias
        FROM code_repositories
        WHERE repository_id = ?1
        ORDER BY alias ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn repository_cleanup_scopes(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT source_scope
        FROM code_repository_scopes
        WHERE repository_id = ?1
        UNION
        SELECT source_scope
        FROM code_repository_index_tasks
        WHERE repository_id = ?1
        UNION
        SELECT source_scope
        FROM code_repository_index_checkpoints
        WHERE repository_id = ?1
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn delete_scope_lifecycle_projection(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    for table in [
        "software_build_targets",
        "software_iac_resources",
        "software_design_elements",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }

    Ok(())
}

fn affected_repository_sets(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT DISTINCT set_id
        FROM code_repository_set_members
        WHERE repository_id = ?1
        ORDER BY set_id ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn count_repository_set_members(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_set_members WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn count_index_tasks(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_index_tasks WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn invalidate_repository_sets(
    transaction: &Transaction<'_>,
    set_ids: &[String],
) -> Result<(), StorageError> {
    for set_id in unique_set_ids(set_ids) {
        transaction.execute(
            "DELETE FROM code_repository_cross_edges WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_set_refresh_tasks WHERE set_id = ?1",
            params![set_id],
        )?;
        transaction.execute(
            "
            UPDATE code_repository_sets
            SET updated_at_ms = strftime('%s','now') * 1000
            WHERE set_id = ?1
            ",
            params![set_id],
        )?;
    }

    Ok(())
}

fn reject_running_repository_set_refresh_tasks(
    transaction: &Transaction<'_>,
    set_ids: &[String],
    now_ms: u64,
) -> Result<(), StorageError> {
    for set_id in unique_set_ids(set_ids) {
        let running_task_id = transaction
            .query_row(
                "
                SELECT task_id
                FROM code_repository_set_refresh_tasks
                WHERE set_id = ?1
                  AND state = 'running'
                  AND lease_expires_at_ms > ?2
                ORDER BY lease_expires_at_ms DESC, created_at_ms ASC, task_id ASC
                LIMIT 1
                ",
                params![set_id, now_ms],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(task_id) = running_task_id {
            return Err(StorageError::InvalidInput(format!(
                "code repository set '{set_id}' has running refresh task '{task_id}'; wait for the task to finish before removing repository members"
            )));
        }
    }

    Ok(())
}

fn unique_set_ids(set_ids: &[String]) -> BTreeSet<&str> {
    set_ids.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;

    #[test]
    fn missing_repository_returns_none() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        create_minimal_schema(&connection);

        let removed =
            remove_repository(&mut connection, "missing", 100).expect("remove should query");

        assert!(removed.is_none());
    }

    #[test]
    fn repository_remove_deletes_index_aliases_tasks_and_invalidates_sets() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        create_minimal_schema(&connection);
        insert_fixture_repository(&connection, "repo", "app", "scope-a");
        insert_fixture_repository(&connection, "other", "svc", "scope-b");
        connection
            .execute(
                "INSERT INTO code_repository_aliases (alias, repository_id) VALUES ('app-session', 'repo')",
                [],
            )
            .expect("secondary alias should insert");
        insert_scope_rows(&connection, "repo", "scope-a", "src/app.rs");
        insert_scope_rows(&connection, "repo", "scope-pending", "src/pending.rs");
        insert_scope_rows(&connection, "other", "scope-b", "src/svc.rs");
        insert_search_row(&connection, "scope-a", "app-symbol");
        insert_search_row(&connection, "scope-pending", "pending-symbol");
        insert_search_row(&connection, "scope-b", "svc-symbol");
        insert_index_task(&connection, "repo", "task-a", "scope-a");
        insert_index_task(&connection, "repo", "task-pending", "scope-pending");
        insert_checkpoint(&connection, "repo", "scope-pending");
        insert_repository_set_fixture(&connection);

        let removed =
            remove_repository(&mut connection, "app-session", 100).expect("remove should succeed");
        let summary = removed.expect("repository should be removed");

        assert_eq!(summary.repository_id, "repo");
        assert_eq!(
            summary.aliases_removed,
            vec!["app".to_owned(), "app-session".to_owned()]
        );
        assert_eq!(summary.removed_scope_count, 2);
        assert_eq!(summary.removed_index_task_count, 2);
        assert_eq!(summary.removed_repository_set_member_count, 1);
        assert_eq!(summary.invalidated_repository_set_count, 1);
        assert_eq!(
            count_where(&connection, "code_repositories", "repository_id = 'repo'"),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_aliases",
                "repository_id = 'repo'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_scopes",
                "repository_id = 'repo'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_index_tasks",
                "repository_id = 'repo'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_files",
                "source_scope = 'scope-a'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "software_components",
                "source_scope = 'scope-a'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "software_build_targets",
                "source_scope = 'scope-a'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_search",
                "source_scope = 'scope-pending'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_search_metadata",
                "source_scope = 'scope-pending'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "software_build_targets",
                "source_scope = 'scope-pending'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_index_checkpoints",
                "repository_id = 'repo'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_search",
                "source_scope = 'scope-a'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_set_members",
                "repository_id = 'repo'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_cross_edges",
                "set_id = 'set-workspace'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_set_overlay_status",
                "set_id = 'set-workspace'"
            ),
            0
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_set_refresh_tasks",
                "set_id = 'set-workspace'"
            ),
            0
        );

        assert_eq!(
            count_where(&connection, "code_repositories", "repository_id = 'other'"),
            1
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_files",
                "source_scope = 'scope-b'"
            ),
            1
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_search",
                "source_scope = 'scope-b'"
            ),
            1
        );
    }

    #[test]
    fn repository_remove_rejects_live_index_task_even_when_older_task_is_queued() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        create_minimal_schema(&connection);
        insert_fixture_repository(&connection, "repo", "app", "scope-a");
        insert_index_task(&connection, "repo", "task-old", "scope-old");
        insert_running_index_task(&connection, "repo", "task-running", "scope-running", 500);

        let error =
            remove_repository(&mut connection, "app", 200).expect_err("live task should reject");

        assert!(error.to_string().contains("task-running"));
        assert_eq!(
            count_where(&connection, "code_repositories", "repository_id = 'repo'"),
            1
        );
        assert_eq!(
            count_where(
                &connection,
                "code_repository_index_tasks",
                "repository_id = 'repo'"
            ),
            2
        );
    }

    #[test]
    fn repository_remove_rejects_live_repository_set_refresh_task() {
        let mut connection = Connection::open_in_memory().expect("connection should open");
        create_minimal_schema(&connection);
        insert_fixture_repository(&connection, "repo", "app", "scope-a");
        insert_fixture_repository(&connection, "other", "svc", "scope-b");
        insert_repository_set_fixture_with_refresh_task(&connection, "running", Some(500));

        let error = remove_repository(&mut connection, "app", 200)
            .expect_err("live set task should reject");

        assert!(error.to_string().contains("refresh-set-workspace"));
        assert_eq!(
            count_where(
                &connection,
                "code_repository_set_refresh_tasks",
                "set_id = 'set-workspace'"
            ),
            1
        );
        assert_eq!(
            count_where(&connection, "code_repositories", "repository_id = 'repo'"),
            1
        );
    }

    fn create_minimal_schema(connection: &Connection) {
        connection
            .execute_batch(
                "
                CREATE TABLE code_repositories (
                    repository_id TEXT PRIMARY KEY,
                    alias TEXT NOT NULL UNIQUE,
                    root_path TEXT NOT NULL,
                    path_filters_json TEXT NOT NULL,
                    language_filters_json TEXT NOT NULL,
                    last_indexed_scope_id TEXT,
                    last_indexed_commit TEXT,
                    tree_hash TEXT,
                    state TEXT NOT NULL,
                    indexed_file_count INTEGER NOT NULL,
                    symbol_count INTEGER NOT NULL,
                    reference_count INTEGER NOT NULL,
                    chunk_count INTEGER NOT NULL,
                    stale INTEGER NOT NULL,
                    degraded_reason TEXT
                );
                CREATE TABLE code_repository_aliases (
                    alias TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL
                );
                CREATE TABLE code_repository_scopes (
                    source_scope TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL,
                    resolved_commit_sha TEXT NOT NULL,
                    tree_hash TEXT NOT NULL,
                    path_filters_json TEXT NOT NULL,
                    language_filters_json TEXT NOT NULL,
                    indexed_file_count INTEGER NOT NULL,
                    symbol_count INTEGER NOT NULL,
                    reference_count INTEGER NOT NULL,
                    chunk_count INTEGER NOT NULL,
                    stale INTEGER NOT NULL,
                    degraded_reason TEXT
                );
                CREATE TABLE code_repository_files (repository_id TEXT NOT NULL, source_scope TEXT NOT NULL, file_id TEXT NOT NULL, path TEXT NOT NULL, language_id TEXT NOT NULL, blob_hash TEXT NOT NULL, byte_len INTEGER NOT NULL, line_count INTEGER NOT NULL, parse_status TEXT NOT NULL, degraded_reason TEXT);
                CREATE TABLE code_repository_symbols (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_references (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_imports (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_dependencies (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_feature_flags (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_calls (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_chunks (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_file_diagnostics (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_path_tombstones (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_index_checkpoints (source_scope TEXT PRIMARY KEY, repository_id TEXT NOT NULL);
                CREATE TABLE code_repository_index_tasks (
                    task_id TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    state TEXT NOT NULL DEFAULT 'queued',
                    lease_expires_at_ms INTEGER,
                    created_at_ms INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE software_components (source_scope TEXT NOT NULL);
                CREATE TABLE software_dependency_usages (source_scope TEXT NOT NULL);
                CREATE TABLE software_sdk_usages (source_scope TEXT NOT NULL);
                CREATE TABLE software_files (source_scope TEXT NOT NULL);
                CREATE TABLE software_topics (source_scope TEXT NOT NULL);
                CREATE TABLE software_relationships (source_scope TEXT NOT NULL);
                CREATE TABLE software_build_targets (source_scope TEXT NOT NULL);
                CREATE TABLE software_iac_resources (source_scope TEXT NOT NULL);
                CREATE TABLE software_design_elements (source_scope TEXT NOT NULL);
                CREATE TABLE software_global_status (source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_sets (set_id TEXT PRIMARY KEY, updated_at_ms INTEGER NOT NULL);
                CREATE TABLE code_repository_set_members (set_id TEXT NOT NULL, repository_id TEXT NOT NULL, source_scope TEXT NOT NULL);
                CREATE TABLE code_repository_cross_edges (set_id TEXT NOT NULL);
                CREATE TABLE code_repository_set_overlay_status (set_id TEXT NOT NULL);
                CREATE TABLE code_repository_set_refresh_tasks (
                    task_id TEXT PRIMARY KEY,
                    set_id TEXT NOT NULL,
                    state TEXT NOT NULL,
                    lease_expires_at_ms INTEGER,
                    created_at_ms INTEGER NOT NULL DEFAULT 0
                );
                CREATE VIRTUAL TABLE code_repository_search USING fts5(source_scope UNINDEXED, document_kind UNINDEXED, record_id UNINDEXED, path UNINDEXED, language_id UNINDEXED, content);
                CREATE TABLE code_repository_search_metadata (source_scope TEXT NOT NULL, document_kind TEXT NOT NULL, record_id TEXT NOT NULL, path TEXT NOT NULL, search_rowid INTEGER NOT NULL UNIQUE, PRIMARY KEY (source_scope, document_kind, record_id));
                ",
            )
            .expect("schema should create");
    }

    fn insert_fixture_repository(
        connection: &Connection,
        repository_id: &str,
        alias: &str,
        source_scope: &str,
    ) {
        connection
            .execute(
                "
                INSERT INTO code_repositories (
                    repository_id, alias, root_path, path_filters_json, language_filters_json,
                    last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                    indexed_file_count, symbol_count, reference_count, chunk_count, stale,
                    degraded_reason
                )
                VALUES (?1, ?2, '/tmp/repo', '[]', '[]', ?3, 'commit', 'tree', 'indexed', 1, 1, 0, 1, 0, NULL)
                ",
                params![repository_id, alias, source_scope],
            )
            .expect("repository should insert");
        connection
            .execute(
                "INSERT INTO code_repository_aliases (alias, repository_id) VALUES (?1, ?2)",
                params![alias, repository_id],
            )
            .expect("alias should insert");
        connection
            .execute(
                "
                INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale, degraded_reason
                )
                VALUES (?1, ?2, 'commit', 'tree', '[]', '[]', 1, 1, 0, 1, 0, NULL)
                ",
                params![source_scope, repository_id],
            )
            .expect("scope should insert");
    }

    fn insert_scope_rows(connection: &Connection, repository_id: &str, scope: &str, path: &str) {
        connection
            .execute(
                "
                INSERT INTO code_repository_files (
                    repository_id, source_scope, file_id, path, language_id, blob_hash,
                    byte_len, line_count, parse_status, degraded_reason
                )
                VALUES (?1, ?2, ?3, ?4, 'rust', 'blob', 1, 1, 'parsed', NULL)
                ",
                params![repository_id, scope, format!("file-{scope}"), path],
            )
            .expect("file should insert");
        for table in [
            "software_components",
            "software_dependency_usages",
            "software_sdk_usages",
            "software_files",
            "software_topics",
            "software_relationships",
            "software_build_targets",
            "software_iac_resources",
            "software_design_elements",
            "software_global_status",
        ] {
            connection
                .execute(
                    &format!("INSERT INTO {table} (source_scope) VALUES (?1)"),
                    params![scope],
                )
                .expect("software row should insert");
        }
    }

    fn insert_search_row(connection: &Connection, scope: &str, record_id: &str) {
        let inserted_count = connection
            .execute(
                "
                INSERT INTO code_repository_search (
                    source_scope, document_kind, record_id, path, language_id, content
                )
                VALUES (?1, 'symbol', ?2, 'src/lib.rs', 'rust', 'target')
                ",
                params![scope, record_id],
            )
            .expect("search row should insert");
        connection
            .execute(
                "
                INSERT INTO code_repository_search_metadata (
                    source_scope, document_kind, record_id, path, search_rowid
                )
                SELECT source_scope, document_kind, record_id, path, rowid
                FROM code_repository_search
                WHERE source_scope = ?1 AND record_id = ?2
                ",
                params![scope, record_id],
            )
            .expect("metadata row should insert");
        assert_eq!(inserted_count, 1);
    }

    fn insert_index_task(connection: &Connection, repository_id: &str, task_id: &str, scope: &str) {
        connection
            .execute(
                "
                INSERT INTO code_repository_index_tasks (
                    task_id, repository_id, source_scope, state, lease_expires_at_ms, created_at_ms
                )
                VALUES (?1, ?2, ?3, 'queued', NULL, 0)
                ",
                params![task_id, repository_id, scope],
            )
            .expect("task should insert");
    }

    fn insert_running_index_task(
        connection: &Connection,
        repository_id: &str,
        task_id: &str,
        scope: &str,
        lease_expires_at_ms: u64,
    ) {
        connection
            .execute(
                "
                INSERT INTO code_repository_index_tasks (
                    task_id, repository_id, source_scope, state, lease_expires_at_ms, created_at_ms
                )
                VALUES (?1, ?2, ?3, 'running', ?4, 1)
                ",
                params![task_id, repository_id, scope, lease_expires_at_ms],
            )
            .expect("running task should insert");
    }

    fn insert_checkpoint(connection: &Connection, repository_id: &str, scope: &str) {
        connection
            .execute(
                "INSERT INTO code_repository_index_checkpoints (source_scope, repository_id) VALUES (?1, ?2)",
                params![scope, repository_id],
            )
            .expect("checkpoint should insert");
    }

    fn insert_repository_set_fixture(connection: &Connection) {
        insert_repository_set_fixture_with_refresh_task(connection, "queued", None);
    }

    fn insert_repository_set_fixture_with_refresh_task(
        connection: &Connection,
        refresh_state: &str,
        lease_expires_at_ms: Option<u64>,
    ) {
        connection
            .execute(
                "INSERT INTO code_repository_sets (set_id, updated_at_ms) VALUES ('set-workspace', 0)",
                [],
            )
            .expect("set should insert");
        connection
            .execute(
                "INSERT INTO code_repository_set_members (set_id, repository_id, source_scope) VALUES ('set-workspace', 'repo', 'scope-a'), ('set-workspace', 'other', 'scope-b')",
                [],
            )
            .expect("members should insert");
        connection
            .execute(
                "INSERT INTO code_repository_cross_edges (set_id) VALUES ('set-workspace')",
                [],
            )
            .expect("edge should insert");
        connection
            .execute(
                "INSERT INTO code_repository_set_overlay_status (set_id) VALUES ('set-workspace')",
                [],
            )
            .expect("overlay should insert");
        connection
            .execute(
                "
                INSERT INTO code_repository_set_refresh_tasks (
                    task_id, set_id, state, lease_expires_at_ms, created_at_ms
                )
                VALUES ('refresh-set-workspace', 'set-workspace', ?1, ?2, 0)
                ",
                params![refresh_state, lease_expires_at_ms],
            )
            .expect("refresh task should insert");
    }

    fn count_where(connection: &Connection, table: &str, predicate: &str) -> usize {
        connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
                [],
                |row| row.get(0),
            )
            .expect("count should load")
    }
}
