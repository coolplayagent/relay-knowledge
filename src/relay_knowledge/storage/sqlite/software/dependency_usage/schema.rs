use rusqlite::Connection;

use crate::storage::StorageError;

pub(in crate::storage::sqlite::software) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    let had_usage_table = dependency_usage_table_exists(connection)?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS software_dependency_usages (
            usage_id TEXT PRIMARY KEY,
            component_id TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            package_name TEXT NOT NULL,
            language_id TEXT NOT NULL,
            module TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_dependency_usages_scope
            ON software_dependency_usages(source_scope, language_id, ecosystem, package_name);
        ",
    )?;
    if !had_usage_table {
        mark_existing_projection_statuses_stale(connection)?;
    }

    Ok(())
}

fn dependency_usage_table_exists(connection: &Connection) -> Result<bool, StorageError> {
    let exists = connection.query_row(
        "
        SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table'
              AND name = 'software_dependency_usages'
        )
        ",
        [],
        |row| row.get::<_, i64>(0),
    )?;

    Ok(exists != 0)
}

fn mark_existing_projection_statuses_stale(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE software_global_status
        SET stale = 1,
            last_error = COALESCE(
                last_error,
                'software dependency usage projection requires refresh'
            )
        ",
        [],
    )?;

    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
