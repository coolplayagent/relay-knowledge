use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn initialize_retention_schema(connection: &Connection) -> Result<(), StorageError> {
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_scopes",
        "retiring",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    replace_legacy_retention_activity_triggers(connection)?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_scope_gc_jobs (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            phase TEXT NOT NULL,
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

        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_repository_insert
        AFTER INSERT ON code_repositories BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_repository_scope_update
        AFTER UPDATE OF last_indexed_scope_id ON code_repositories BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_scope_insert
        AFTER INSERT ON code_repository_scopes BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_scope_delete
        AFTER DELETE ON code_repository_scopes BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_scope_update
        AFTER UPDATE OF repository_id, source_scope, retiring ON code_repository_scopes BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_task_insert
        AFTER INSERT ON code_repository_index_tasks WHEN NEW.state = 'succeeded' BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_task_delete
        AFTER DELETE ON code_repository_index_tasks WHEN OLD.state = 'succeeded' BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_task_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_tasks
        WHEN OLD.state = 'succeeded' OR NEW.state = 'succeeded' BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_checkpoint_insert
        AFTER INSERT ON code_repository_index_checkpoints
        WHEN NEW.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_checkpoint_delete
        AFTER DELETE ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
        END;
        CREATE TRIGGER IF NOT EXISTS code_repository_retention_activity_checkpoint_update
        AFTER UPDATE OF repository_id, source_scope, state, updated_at_ms
        ON code_repository_index_checkpoints
        WHEN OLD.state IN ('complete', 'completed')
          OR NEW.state IN ('complete', 'completed') BEGIN
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT OLD.repository_id
            WHERE EXISTS (
                SELECT 1 FROM code_repositories
                WHERE repository_id = OLD.repository_id
            ) AND NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = OLD.repository_id
            );
            INSERT INTO code_repository_retention_activity_dirty (repository_id)
            SELECT NEW.repository_id
            WHERE NOT EXISTS (
                SELECT 1 FROM code_repository_retention_activity_dirty
                WHERE repository_id = NEW.repository_id
            );
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
    Ok(())
}

fn replace_legacy_retention_activity_triggers(connection: &Connection) -> Result<(), StorageError> {
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
    connection.execute_batch(
        "DROP TRIGGER IF EXISTS code_repository_retention_activity_repository_insert;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_repository_scope_update;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_insert;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_delete;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_update;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_task_insert;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_task_delete;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_task_update;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_insert;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_delete;
         DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_update;",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "retention_schema_tests.rs"]
mod tests;
