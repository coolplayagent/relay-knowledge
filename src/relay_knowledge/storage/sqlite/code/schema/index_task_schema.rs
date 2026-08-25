use rusqlite::Connection;

use crate::storage::StorageError;

use super::migrations::table_has_columns;

#[cfg(test)]
#[path = "index_task_schema_tests.rs"]
mod tests;

const INCREMENTAL_CLONE_OWNER_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS code_repository_incremental_clone_progress (
        source_scope TEXT NOT NULL PRIMARY KEY,
        repository_id TEXT NOT NULL,
        base_scope TEXT NOT NULL,
        task_id TEXT NOT NULL,
        delta_digest TEXT NOT NULL,
        protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
        phase TEXT NOT NULL CHECK (
            phase IN ('tables', 'search', 'clone_complete')
        ),
        table_ordinal INTEGER NOT NULL CHECK (table_ordinal >= 0),
        completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
        cursor_key TEXT,
        cursor_tiebreaker TEXT,
        completed_table_ordinal INTEGER CHECK (completed_table_ordinal >= 0),
        expected_table_rows INTEGER CHECK (expected_table_rows >= 0),
        scanned_table_rows INTEGER NOT NULL CHECK (scanned_table_rows >= 0),
        copied_table_rows INTEGER NOT NULL CHECK (copied_table_rows >= 0),
        scanned_total_rows INTEGER NOT NULL CHECK (scanned_total_rows >= 0),
        copied_total_rows INTEGER NOT NULL CHECK (copied_total_rows >= 0),
        copied_total_bytes INTEGER NOT NULL CHECK (copied_total_bytes >= 0),
        cloned_file_count INTEGER NOT NULL CHECK (cloned_file_count >= 0),
        cloned_symbol_count INTEGER NOT NULL CHECK (cloned_symbol_count >= 0),
        cloned_reference_count INTEGER NOT NULL CHECK (cloned_reference_count >= 0),
        cloned_chunk_count INTEGER NOT NULL CHECK (cloned_chunk_count >= 0),
        cloned_diagnostic_count INTEGER NOT NULL CHECK (cloned_diagnostic_count >= 0),
        cloned_reference_group_count INTEGER NOT NULL
            CHECK (cloned_reference_group_count >= 0),
        cloned_search_document_count INTEGER NOT NULL
            CHECK (cloned_search_document_count >= 0),
        base_manifest_reference_count INTEGER NOT NULL
            CHECK (base_manifest_reference_count >= 0),
        base_manifest_group_count INTEGER NOT NULL
            CHECK (base_manifest_group_count >= 0),
        scanned_reference_occurrence_count INTEGER NOT NULL
            CHECK (scanned_reference_occurrence_count >= 0),
        scanned_reference_row_count INTEGER NOT NULL
            CHECK (scanned_reference_row_count >= 0),
        scanned_reference_group_count INTEGER NOT NULL
            CHECK (scanned_reference_group_count >= 0),
        scanned_reference_search_owner_count INTEGER NOT NULL
            CHECK (scanned_reference_search_owner_count >= 0),
        base_source_fact_row_upper_bound INTEGER NOT NULL
            CHECK (base_source_fact_row_upper_bound > 0),
        page_row_limit INTEGER NOT NULL CHECK (page_row_limit > 0),
        page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0),
        updated_at_ms INTEGER NOT NULL,
        FOREIGN KEY (source_scope) REFERENCES code_repository_scopes(source_scope)
            ON DELETE CASCADE,
        FOREIGN KEY (base_scope) REFERENCES code_repository_scopes(source_scope)
            ON DELETE RESTRICT
    );

    CREATE TABLE IF NOT EXISTS code_repository_incremental_clone_affected_paths (
        source_scope TEXT NOT NULL,
        path TEXT NOT NULL,
        PRIMARY KEY (source_scope, path),
        FOREIGN KEY (source_scope)
            REFERENCES code_repository_incremental_clone_progress(source_scope)
            ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS code_repository_incremental_clone_progress_task
        ON code_repository_incremental_clone_progress(task_id, source_scope);
";

pub(super) fn initialize_index_task_schema(connection: &Connection) -> Result<(), StorageError> {
    repair_empty_reference_search_owner_schema(connection)?;
    repair_empty_reference_resolution_progress_schema(connection)?;
    let legacy_reference_search_progress = table_has_columns(
        connection,
        "code_repository_reference_search_progress",
        &[
            "source_scope",
            "stage",
            "completed_page_ordinal",
            "cleanup_cursor_rowid",
            "build_cursor_reference_id",
            "cleanup_total_count",
            "build_total_count",
            "cleaned_count",
            "built_count",
            "page_document_limit",
            "page_byte_limit",
        ],
    )? && !table_has_columns(
        connection,
        "code_repository_reference_search_progress",
        &["projection_version"],
    )?;
    if !legacy_reference_search_progress {
        repair_empty_reference_search_progress_schema(connection)?;
    }
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_index_checkpoints (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            state TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            tree_hash TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            total_path_count INTEGER NOT NULL,
            parsed_file_count INTEGER NOT NULL,
            committed_file_count INTEGER NOT NULL,
            committed_symbol_count INTEGER NOT NULL,
            committed_reference_count INTEGER NOT NULL,
            committed_chunk_count INTEGER NOT NULL,
            committed_fact_row_count INTEGER NOT NULL DEFAULT 0,
            incremental_summary_json TEXT,
            batch_count INTEGER NOT NULL,
            last_path TEXT,
            resource_budget_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            error_message TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_index_tasks (
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
            publication_generation INTEGER NOT NULL DEFAULT 0,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            resource_budget_json TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE,
            UNIQUE (repository_id, input_fingerprint)
        );

        CREATE TABLE IF NOT EXISTS code_repository_index_batch_staging (
            source_scope TEXT NOT NULL,
            batch_index INTEGER NOT NULL,
            state TEXT NOT NULL,
            file_count INTEGER NOT NULL,
            fact_row_count INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (source_scope, batch_index),
            FOREIGN KEY (source_scope) REFERENCES code_repository_index_checkpoints(source_scope)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_reference_search_groups (
            source_scope TEXT NOT NULL,
            group_id TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            target_hint TEXT NOT NULL,
            language_id TEXT NOT NULL,
            occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
            PRIMARY KEY (source_scope, group_id),
            UNIQUE (source_scope, name, kind, path, target_hint)
        );

        CREATE TABLE IF NOT EXISTS code_repository_reference_search_manifests (
            source_scope TEXT NOT NULL PRIMARY KEY,
            projection_version INTEGER NOT NULL CHECK (projection_version > 0),
            reference_count INTEGER NOT NULL CHECK (reference_count >= 0),
            group_count INTEGER NOT NULL CHECK (group_count >= 0)
        );

        CREATE TABLE IF NOT EXISTS code_repository_reference_search_progress (
            source_scope TEXT NOT NULL PRIMARY KEY,
            projection_version INTEGER NOT NULL CHECK (projection_version > 0),
            stage TEXT NOT NULL CHECK (stage IN ('cleanup', 'discover', 'build')),
            completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
            cleanup_cursor_rowid INTEGER,
            cleanup_cursor_record_id TEXT,
            discovery_cursor_reference_id TEXT,
            build_cursor_group_id TEXT,
            expected_reference_count INTEGER NOT NULL CHECK (expected_reference_count >= 0),
            cleanup_total_count INTEGER NOT NULL CHECK (cleanup_total_count >= 0),
            discovered_reference_count INTEGER NOT NULL CHECK (discovered_reference_count >= 0),
            discovered_group_count INTEGER NOT NULL CHECK (discovered_group_count >= 0),
            build_total_count INTEGER NOT NULL CHECK (build_total_count >= 0),
            cleaned_count INTEGER NOT NULL CHECK (cleaned_count >= 0),
            built_count INTEGER NOT NULL CHECK (built_count >= 0),
            page_document_limit INTEGER NOT NULL CHECK (page_document_limit > 0),
            page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0),
            FOREIGN KEY (source_scope) REFERENCES code_repository_index_checkpoints(source_scope)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_reference_resolution_progress (
            source_scope TEXT NOT NULL PRIMARY KEY,
            protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
            stage TEXT NOT NULL CHECK (stage = 'resolve'),
            completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
            cursor_reference_id TEXT,
            expected_reference_count INTEGER NOT NULL CHECK (expected_reference_count >= 0),
            resolved_reference_count INTEGER NOT NULL CHECK (resolved_reference_count >= 0),
            page_document_limit INTEGER NOT NULL
                CHECK (page_document_limit > 0 AND page_document_limit <= 32768),
            page_byte_limit INTEGER NOT NULL
                CHECK (page_byte_limit > 0 AND page_byte_limit <= 16777216),
            CHECK (resolved_reference_count <= expected_reference_count),
            FOREIGN KEY (source_scope) REFERENCES code_repository_index_checkpoints(source_scope)
                ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS code_repository_index_batch_staging_state
            ON code_repository_index_batch_staging(source_scope, state, batch_index);
        CREATE INDEX IF NOT EXISTS code_repository_reference_search_groups_path
            ON code_repository_reference_search_groups(source_scope, path, group_id);
        CREATE INDEX IF NOT EXISTS code_repository_index_checkpoints_repository_scope
            ON code_repository_index_checkpoints(repository_id, source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_index_checkpoints_publication_retention
            ON code_repository_index_checkpoints(
                repository_id, state, updated_at_ms DESC, source_scope DESC
            );

        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_claimable
            ON code_repository_index_tasks(state, next_retry_at_ms, created_at_ms);
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_repository
            ON code_repository_index_tasks(repository_id, state, created_at_ms);
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_repository_fifo
            ON code_repository_index_tasks(repository_id, created_at_ms, task_id);
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_audit_retention
            ON code_repository_index_tasks(
                repository_id, state, updated_at_ms DESC, created_at_ms DESC, task_id DESC
            );
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_scope_retention
            ON code_repository_index_tasks(
                source_scope, state, updated_at_ms ASC, task_id ASC
            );

        CREATE TABLE IF NOT EXISTS code_repository_publication_fences (
            repository_id TEXT PRIMARY KEY,
            generation INTEGER NOT NULL,
            task_id TEXT NOT NULL,
            attempt_count INTEGER NOT NULL,
            lease_owner TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_publication_receipts (
            task_id TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            publication_generation INTEGER NOT NULL,
            published_at_ms INTEGER NOT NULL,
            PRIMARY KEY (task_id, publication_generation),
            FOREIGN KEY (task_id) REFERENCES code_repository_index_tasks(task_id)
                ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS code_repository_publication_receipts_scope
            ON code_repository_publication_receipts(repository_id, source_scope);

        ",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_index_checkpoints",
        "committed_fact_row_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_index_checkpoints",
        "incremental_summary_json",
        "TEXT",
    )?;
    repair_empty_incremental_clone_progress_schema(connection)?;
    connection.execute_batch(INCREMENTAL_CLONE_OWNER_SCHEMA)?;
    if !crate::storage::sqlite::schema::marker::reference_search_group_schema_is_current(
        connection,
    )? {
        return Err(StorageError::Invariant(
            "reference-search group owner schema is not current after initialization".to_owned(),
        ));
    }
    if legacy_reference_search_progress {
        migrate_reference_search_progress_v1(connection)?;
    }
    if !crate::storage::sqlite::schema::marker::reference_search_progress_schema_is_current(
        connection,
    )? {
        return Err(StorageError::Invariant(
            "reference-search progress schema is not current after initialization".to_owned(),
        ));
    }
    if !crate::storage::sqlite::schema::marker::reference_resolution_progress_schema_is_current(
        connection,
    )? {
        return Err(StorageError::Invariant(
            "reference-resolution progress schema is not current after initialization".to_owned(),
        ));
    }
    if !crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(connection)? {
        return Err(StorageError::Invariant(
            "incremental-clone progress schema is not current after initialization".to_owned(),
        ));
    }
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_index_tasks",
        "publication_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_publication_retention
            ON code_repository_index_tasks(
                repository_id, state, publication_generation DESC,
                updated_at_ms DESC, created_at_ms DESC, task_id DESC, source_scope
            );
        ",
    )?;
    Ok(())
}

fn repair_empty_incremental_clone_progress_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    let (progress_exists, paths_exist) = connection.query_row(
        "SELECT
             EXISTS (SELECT 1 FROM sqlite_master
                     WHERE type = 'table'
                       AND name = 'code_repository_incremental_clone_progress'),
             EXISTS (SELECT 1 FROM sqlite_master
                     WHERE type = 'table'
                       AND name = 'code_repository_incremental_clone_affected_paths')",
        [],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
    )?;
    if !progress_exists && !paths_exist {
        return Ok(());
    }
    if progress_exists
        && paths_exist
        && crate::storage::sqlite::schema::incremental_clone_marker::schema_is_current(connection)?
    {
        return Ok(());
    }
    let progress_has_rows = progress_exists
        && connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_incremental_clone_progress LIMIT 1
             )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
    let paths_have_rows = paths_exist
        && connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_incremental_clone_affected_paths LIMIT 1
             )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
    if progress_has_rows || paths_have_rows {
        return Err(StorageError::Invariant(
            "non-empty incremental-clone owner tables have an incompatible schema".to_owned(),
        ));
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "DROP INDEX IF EXISTS code_repository_incremental_clone_progress_task;
         DROP TABLE IF EXISTS code_repository_incremental_clone_affected_paths;
         DROP TABLE IF EXISTS code_repository_incremental_clone_progress;",
    )?;
    transaction.commit().map_err(StorageError::from)
}

fn repair_empty_reference_resolution_progress_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    let table = "code_repository_reference_resolution_progress";
    let exists = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists
        || crate::storage::sqlite::schema::marker::reference_resolution_progress_schema_is_current(
            connection,
        )?
    {
        return Ok(());
    }
    let has_rows = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_reference_resolution_progress LIMIT 1
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if has_rows {
        return Err(StorageError::Invariant(
            "non-empty reference-resolution progress table has an incompatible schema".to_owned(),
        ));
    }
    connection.execute(
        "DROP TABLE code_repository_reference_resolution_progress",
        [],
    )?;
    Ok(())
}

fn repair_empty_reference_search_progress_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
                        WHERE type = 'table'
                          AND name = 'code_repository_reference_search_progress')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists
        || crate::storage::sqlite::schema::marker::reference_search_progress_schema_is_current(
            connection,
        )?
    {
        return Ok(());
    }
    let has_rows = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_reference_search_progress LIMIT 1)",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if has_rows {
        return Err(StorageError::Invariant(
            "non-empty reference-search progress table has an incompatible schema".to_owned(),
        ));
    }
    connection.execute("DROP TABLE code_repository_reference_search_progress", [])?;
    Ok(())
}

fn repair_empty_reference_search_owner_schema(connection: &Connection) -> Result<(), StorageError> {
    let (groups_exist, manifests_exist) = connection.query_row(
        "SELECT
             EXISTS (SELECT 1 FROM sqlite_master
                     WHERE type = 'table'
                       AND name = 'code_repository_reference_search_groups'),
             EXISTS (SELECT 1 FROM sqlite_master
                     WHERE type = 'table'
                       AND name = 'code_repository_reference_search_manifests')",
        [],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
    )?;
    if !groups_exist && !manifests_exist {
        return Ok(());
    }
    if crate::storage::sqlite::schema::marker::reference_search_group_schema_is_current(connection)?
    {
        return Ok(());
    }
    let groups_have_rows = groups_exist
        && connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM code_repository_reference_search_groups LIMIT 1)",
            [],
            |row| row.get::<_, bool>(0),
        )?;
    let manifests_have_rows = manifests_exist
        && connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM code_repository_reference_search_manifests LIMIT 1)",
            [],
            |row| row.get::<_, bool>(0),
        )?;
    if groups_have_rows || manifests_have_rows {
        return Err(StorageError::Invariant(
            "non-empty reference-search group owner tables have an incompatible schema".to_owned(),
        ));
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "DROP INDEX IF EXISTS code_repository_reference_search_groups_path;
         DROP TABLE IF EXISTS code_repository_reference_search_groups;
         DROP TABLE IF EXISTS code_repository_reference_search_manifests;
         CREATE TABLE code_repository_reference_search_groups (
             source_scope TEXT NOT NULL,
             group_id TEXT NOT NULL,
             name TEXT NOT NULL,
             kind TEXT NOT NULL,
             path TEXT NOT NULL,
             target_hint TEXT NOT NULL,
             language_id TEXT NOT NULL,
             occurrence_count INTEGER NOT NULL CHECK (occurrence_count > 0),
             PRIMARY KEY (source_scope, group_id),
             UNIQUE (source_scope, name, kind, path, target_hint)
         );
         CREATE TABLE code_repository_reference_search_manifests (
             source_scope TEXT NOT NULL PRIMARY KEY,
             projection_version INTEGER NOT NULL CHECK (projection_version > 0),
             reference_count INTEGER NOT NULL CHECK (reference_count >= 0),
             group_count INTEGER NOT NULL CHECK (group_count >= 0)
         );
         CREATE INDEX code_repository_reference_search_groups_path
             ON code_repository_reference_search_groups(source_scope, path, group_id);",
    )?;
    transaction.commit()?;
    if !crate::storage::sqlite::schema::marker::reference_search_group_schema_is_current(
        connection,
    )? {
        return Err(StorageError::Invariant(
            "empty reference-search group owner schema repair did not install the exact current shape"
                .to_owned(),
        ));
    }
    Ok(())
}

fn migrate_reference_search_progress_v1(connection: &Connection) -> Result<(), StorageError> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "ALTER TABLE code_repository_reference_search_progress
             RENAME TO code_repository_reference_search_progress_v1;
         CREATE TABLE code_repository_reference_search_progress (
             source_scope TEXT NOT NULL PRIMARY KEY,
             projection_version INTEGER NOT NULL CHECK (projection_version > 0),
             stage TEXT NOT NULL CHECK (stage IN ('cleanup', 'discover', 'build')),
             completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
             cleanup_cursor_rowid INTEGER,
             cleanup_cursor_record_id TEXT,
             discovery_cursor_reference_id TEXT,
             build_cursor_group_id TEXT,
             expected_reference_count INTEGER NOT NULL CHECK (expected_reference_count >= 0),
             cleanup_total_count INTEGER NOT NULL CHECK (cleanup_total_count >= 0),
             discovered_reference_count INTEGER NOT NULL CHECK (discovered_reference_count >= 0),
             discovered_group_count INTEGER NOT NULL CHECK (discovered_group_count >= 0),
             build_total_count INTEGER NOT NULL CHECK (build_total_count >= 0),
             cleaned_count INTEGER NOT NULL CHECK (cleaned_count >= 0),
             built_count INTEGER NOT NULL CHECK (built_count >= 0),
             page_document_limit INTEGER NOT NULL CHECK (page_document_limit > 0),
             page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0),
             FOREIGN KEY (source_scope) REFERENCES code_repository_index_checkpoints(source_scope)
                 ON DELETE CASCADE
         );
         INSERT INTO code_repository_reference_search_progress (
             source_scope, projection_version, stage, completed_page_ordinal,
             cleanup_cursor_rowid, cleanup_cursor_record_id,
             discovery_cursor_reference_id, build_cursor_group_id,
             expected_reference_count, cleanup_total_count, discovered_reference_count,
             discovered_group_count, build_total_count, cleaned_count, built_count,
             page_document_limit, page_byte_limit
         )
         SELECT source_scope, 1, stage, completed_page_ordinal,
                cleanup_cursor_rowid, NULL, NULL, build_cursor_reference_id,
                build_total_count, cleanup_total_count, 0, 0, build_total_count,
                cleaned_count, built_count, page_document_limit, page_byte_limit
         FROM code_repository_reference_search_progress_v1;
         DROP TABLE code_repository_reference_search_progress_v1;",
    )?;
    transaction.commit().map_err(StorageError::from)
}
