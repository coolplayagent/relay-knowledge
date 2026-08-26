use rusqlite::Connection;

use crate::storage::StorageError;
use crate::storage::sqlite::schema::marker::{
    REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION, SEARCH_ORPHAN_GC_PHASE_MIGRATION,
};

use super::migrations::{code_schema_migration_applied, mark_code_schema_migration};

#[path = "retention_activity_trigger_schema.rs"]
mod retention_activity_trigger_schema;

pub(super) fn initialize_retention_schema(connection: &Connection) -> Result<(), StorageError> {
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_scopes",
        "retiring",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_scope_gc_jobs (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            search_rowid_cursor INTEGER,
            deleted_rows INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_error TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_retention_jobs (
            repository_id TEXT PRIMARY KEY,
            initial_scope TEXT NOT NULL,
            cutoff_ms INTEGER NOT NULL,
            cutoff_publication_generation INTEGER NOT NULL DEFAULT 0,
            phase TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_error TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_retention_scans (
            scan_id INTEGER PRIMARY KEY CHECK (scan_id = 1),
            max_indexed_repositories INTEGER NOT NULL,
            catalog_revision INTEGER NOT NULL,
            cursor_activity_ms INTEGER NOT NULL,
            cursor_repository_id TEXT NOT NULL,
            eligible_count INTEGER NOT NULL,
            oldest_repository_id TEXT,
            oldest_source_scope TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            CHECK (
                (oldest_repository_id IS NULL AND oldest_source_scope IS NULL)
                OR
                (oldest_repository_id IS NOT NULL AND oldest_source_scope IS NOT NULL)
            )
        );

        CREATE TABLE IF NOT EXISTS code_repository_retention_catalog (
            catalog_id INTEGER PRIMARY KEY CHECK (catalog_id = 1),
            revision INTEGER NOT NULL
        );
        INSERT OR IGNORE INTO code_repository_retention_catalog (catalog_id, revision)
            VALUES (1, 1);

        CREATE TABLE IF NOT EXISTS code_repository_retention_activity (
            repository_id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            activity_ms INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE,
            FOREIGN KEY (source_scope) REFERENCES code_repository_scopes(source_scope)
                ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS code_repository_retention_activity_dirty (
            repository_id TEXT PRIMARY KEY,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE
        );

        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_repository_insert
        AFTER INSERT ON code_repositories BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_repository_delete
        AFTER DELETE ON code_repositories BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_repository_scope_update
        AFTER UPDATE OF last_indexed_scope_id ON code_repositories BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_scope_insert
        AFTER INSERT ON code_repository_scopes BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_scope_delete
        AFTER DELETE ON code_repository_scopes BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_scope_retiring_update
        AFTER UPDATE OF retiring ON code_repository_scopes BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_member_insert
        AFTER INSERT ON code_repository_set_members BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_member_delete
        AFTER DELETE ON code_repository_set_members BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_member_update
        AFTER UPDATE ON code_repository_set_members BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_task_insert
        AFTER INSERT ON code_repository_index_tasks WHEN NEW.state = 'succeeded' BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_task_delete
        AFTER DELETE ON code_repository_index_tasks WHEN OLD.state = 'succeeded' BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_task_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_tasks
        WHEN OLD.state = 'succeeded' OR NEW.state = 'succeeded' BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_checkpoint_insert
        AFTER INSERT ON code_repository_index_checkpoints
        WHEN NEW.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_checkpoint_delete
        AFTER DELETE ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_catalog_checkpoint_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed')
          OR NEW.state IN ('complete', 'completed') BEGIN
            UPDATE code_repository_retention_catalog SET revision = revision + 1
            WHERE catalog_id = 1;
        END;

        CREATE INDEX IF NOT EXISTS code_repository_scope_gc_jobs_repository
            ON code_repository_scope_gc_jobs(repository_id, updated_at_ms, source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_retention_jobs_updated
            ON code_repository_retention_jobs(updated_at_ms, repository_id);
        CREATE INDEX IF NOT EXISTS code_repository_scopes_retention
            ON code_repository_scopes(repository_id, retiring, source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_set_members_repository_scope
            ON code_repository_set_members(repository_id, source_scope, set_id);
        CREATE INDEX IF NOT EXISTS code_repository_retention_activity_order
            ON code_repository_retention_activity(activity_ms, repository_id);
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_scope_activity
            ON code_repository_index_tasks(
                repository_id, source_scope, state, updated_at_ms DESC
            );
        CREATE INDEX IF NOT EXISTS code_repository_index_checkpoints_scope_activity
            ON code_repository_index_checkpoints(
                repository_id, source_scope, state, updated_at_ms DESC
            );
        CREATE INDEX IF NOT EXISTS code_repository_cross_edges_from_scope_gc
            ON code_repository_cross_edges(from_source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_cross_edges_to_scope_gc
            ON code_repository_cross_edges(to_source_scope);
        ",
    )?;
    connection.execute_batch(retention_activity_trigger_schema::SCHEMA)?;
    connection.execute_batch(
        "
        INSERT INTO code_repository_retention_activity (
            repository_id, source_scope, activity_ms
        )
        SELECT repository.repository_id,
               repository.last_indexed_scope_id,
               MAX(
                   COALESCE((
                       SELECT MAX(task.updated_at_ms)
                       FROM code_repository_index_tasks task
                       WHERE task.repository_id = repository.repository_id
                         AND task.source_scope = repository.last_indexed_scope_id
                         AND task.state = 'succeeded'
                   ), 0),
                   COALESCE((
                       SELECT MAX(checkpoint.updated_at_ms)
                       FROM code_repository_index_checkpoints checkpoint
                       WHERE checkpoint.repository_id = repository.repository_id
                         AND checkpoint.source_scope = repository.last_indexed_scope_id
                         AND checkpoint.state IN ('complete', 'completed')
                   ), 0)
               )
        FROM code_repositories repository
        JOIN code_repository_scopes scope
          ON scope.repository_id = repository.repository_id
         AND scope.source_scope = repository.last_indexed_scope_id
         AND scope.retiring = 0
        WHERE repository.last_indexed_scope_id IS NOT NULL
        ON CONFLICT(repository_id) DO UPDATE SET
            source_scope = excluded.source_scope,
            activity_ms = excluded.activity_ms;
        ",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_retention_jobs",
        "cutoff_publication_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_retention_scans",
        "catalog_revision",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_scope_gc_jobs",
        "search_rowid_cursor",
        "INTEGER",
    )?;
    rewind_legacy_jobs_for_search_orphan_gc_once(connection)?;
    rewind_legacy_jobs_for_reference_search_group_gc_once(connection)?;
    Ok(())
}

fn rewind_legacy_jobs_for_search_orphan_gc_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_ORPHAN_GC_PHASE_MIGRATION)? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "UPDATE code_repository_scope_gc_jobs
         SET phase = 'search_orphans', search_rowid_cursor = NULL
         WHERE phase IN (
             'path_tombstones', 'file_diagnostics', 'chunks', 'calls', 'routes',
             'feature_flags', 'dependencies', 'imports', 'references', 'symbols', 'files',
             'software_components', 'software_dependency_usages', 'software_sdk_usages',
             'software_files', 'software_topics', 'software_relationships',
             'software_global_status', 'software_build_targets', 'software_iac_resources',
             'software_design_elements', 'business_mappings', 'business_term_aliases',
             'business_terms', 'business_domains', 'business_knowledge_status',
             'commit_scopes', 'index_batch_staging',
             'index_task_history', 'checkpoint', 'scope_metadata'
         )",
        [],
    )?;
    mark_code_schema_migration(&transaction, SEARCH_ORPHAN_GC_PHASE_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

fn rewind_legacy_jobs_for_reference_search_group_gc_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION)? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "UPDATE code_repository_scope_gc_jobs
         SET phase = 'reference_search_groups', search_rowid_cursor = NULL
         WHERE phase IN (
             'path_tombstones', 'file_diagnostics', 'chunks', 'calls', 'routes',
             'feature_flags', 'dependencies', 'imports', 'references', 'symbols', 'files',
             'software_components', 'software_dependency_usages', 'software_sdk_usages',
             'software_files', 'software_topics', 'software_relationships',
             'software_global_status', 'software_build_targets', 'software_iac_resources',
             'software_design_elements', 'business_mappings', 'business_term_aliases',
             'business_terms', 'business_domains', 'business_knowledge_status',
             'commit_scopes', 'index_batch_staging',
             'index_task_history', 'checkpoint', 'scope_metadata'
         )",
        [],
    )?;
    mark_code_schema_migration(&transaction, REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn upgrade_legacy_retention_activity_triggers(
    connection: &Connection,
) -> Result<(), StorageError> {
    let legacy_exists = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM sqlite_master
             WHERE type = 'trigger'
               AND name LIKE 'code_repository_retention_activity_%'
               AND sql LIKE '%INSERT OR IGNORE%'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !legacy_exists {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(retention_activity_trigger_schema::DROP_SCHEMA)?;
    transaction.execute_batch(retention_activity_trigger_schema::SCHEMA)?;
    transaction.commit().map_err(StorageError::from)
}

#[cfg(test)]
#[path = "retention_schema_tests.rs"]
mod tests;
