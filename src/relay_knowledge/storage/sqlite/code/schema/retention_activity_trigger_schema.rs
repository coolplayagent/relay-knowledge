pub(super) const DROP_SCHEMA: &str = "
    DROP TRIGGER IF EXISTS code_repository_retention_activity_repository_insert;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_repository_scope_update;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_insert;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_delete;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_scope_update;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_task_insert;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_task_delete;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_task_update;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_insert;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_delete;
    DROP TRIGGER IF EXISTS code_repository_retention_activity_checkpoint_update;
";

pub(super) const SCHEMA: &str = "
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
";
