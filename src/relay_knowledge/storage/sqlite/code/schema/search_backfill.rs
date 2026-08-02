use rusqlite::Connection;

use crate::storage::StorageError;

use super::{
    CALL_SEARCH_SIGNATURE_MIGRATION, SEARCH_BACKFILL_MIGRATION, SEARCH_METADATA_BACKFILL_MIGRATION,
    code_schema_migration_applied, mark_code_schema_migration, table_has_columns,
};

#[cfg(test)]
#[path = "search_backfill_tests.rs"]
mod tests;

pub(super) fn backfill_code_repository_search(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_BACKFILL_MIGRATION)? {
        return Ok(());
    }
    if !code_repository_search_is_empty(connection)? {
        mark_code_schema_migration(connection, SEARCH_BACKFILL_MIGRATION)?;
        return Ok(());
    }
    backfill_search_symbols(connection)?;
    backfill_search_references(connection)?;
    backfill_search_imports(connection)?;
    backfill_search_dependencies(connection)?;
    backfill_search_feature_flags(connection)?;
    backfill_search_calls(connection)?;
    backfill_search_routes(connection)?;
    backfill_search_chunks(connection)?;
    mark_code_schema_migration(connection, SEARCH_BACKFILL_MIGRATION)?;

    Ok(())
}

pub(super) fn backfill_code_repository_search_metadata(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_METADATA_BACKFILL_MIGRATION)? {
        return Ok(());
    }
    sync_code_repository_search_metadata(connection)?;
    mark_code_schema_migration(connection, SEARCH_METADATA_BACKFILL_MIGRATION)
}

fn sync_code_repository_search_metadata(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repository_search_metadata (
            source_scope, document_kind, record_id, path, search_rowid
        )
        SELECT source_scope, document_kind, record_id, path, rowid
        FROM code_repository_search
        ",
        [],
    )?;

    Ok(())
}

fn code_repository_search_is_empty(connection: &Connection) -> Result<bool, StorageError> {
    connection
        .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|count| count == 0)
        .map_err(StorageError::from)
}

fn backfill_search_symbols(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_symbols",
        &[
            "source_scope",
            "symbol_snapshot_id",
            "path",
            "language_id",
            "name",
            "qualified_name",
            "kind",
            "signature",
            "doc_comment",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'symbol', symbol_snapshot_id, path, language_id,
               name || ' ' || qualified_name || ' ' || kind || ' ' || signature || ' ' ||
               coalesce(doc_comment, '') || ' ' || coalesce(symbol_role_json, '')
        FROM code_repository_symbols
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_references(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_references",
        &[
            "source_scope",
            "reference_id",
            "path",
            "name",
            "kind",
            "target_hint",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT reference.source_scope, 'reference', reference.reference_id, reference.path,
               coalesce(file.language_id, ''),
               reference.name || ' ' || reference.kind || ' ' || coalesce(reference.target_hint, '')
        FROM code_repository_references reference
        LEFT JOIN code_repository_files file
          ON file.source_scope = reference.source_scope
         AND file.path = reference.path
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_imports(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_imports",
        &["source_scope", "import_id", "path", "module", "target_hint"],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT import.source_scope, 'import', import.import_id, import.path,
               coalesce(file.language_id, ''),
               import.module || ' ' || coalesce(import.target_hint, '')
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_dependencies(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_dependencies",
        &[
            "source_scope",
            "dependency_id",
            "path",
            "language_id",
            "ecosystem",
            "package_name",
            "requirement",
            "resolved_version",
            "dependency_group",
            "source_kind",
            "excerpt",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'dependency', dependency_id, path, language_id,
               ecosystem || ' ' || package_name || ' ' || coalesce(requirement, '') || ' ' ||
               coalesce(resolved_version, '') || ' ' || dependency_group || ' ' ||
               source_kind || ' ' || excerpt || ' ' || path
        FROM code_repository_dependencies
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_feature_flags(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_feature_flags",
        &[
            "source_scope",
            "usage_id",
            "path",
            "language_id",
            "name",
            "source_kind",
            "source_key",
            "edge_kind",
            "excerpt",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'feature_flag', usage_id, path, language_id,
               name || ' ' || source_kind || ' ' || source_key || ' ' || edge_kind || ' ' ||
               excerpt || ' ' || path
        FROM code_repository_feature_flags
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_calls(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_calls",
        &[
            "source_scope",
            "call_id",
            "path",
            "caller_name",
            "callee_name",
            "target_hint",
        ],
    )? {
        return Ok(());
    }
    insert_search_calls(connection)
}

fn backfill_search_routes(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_routes",
        &[
            "source_scope",
            "route_id",
            "path",
            "language_id",
            "url",
            "http_method",
            "handler_name",
            "framework",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'route', route_id, path, language_id,
               url || ' ' || http_method || ' ' || handler_name || ' ' || framework || ' ' || path
        FROM code_repository_routes
        ",
        [],
    )?;

    Ok(())
}

pub(super) fn rebuild_call_search_documents_after_signature_upgrade(
    connection: &Connection,
) -> Result<(), StorageError> {
    if !call_search_supports_symbol_signatures(connection)?
        || code_schema_migration_applied(connection, CALL_SEARCH_SIGNATURE_MIGRATION)?
    {
        return Ok(());
    }

    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = rebuild_call_search_documents_with_migration_marker(connection);
    match result {
        Ok(()) => connection
            .execute_batch("COMMIT")
            .map_err(StorageError::from),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn rebuild_call_search_documents_with_migration_marker(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, CALL_SEARCH_SIGNATURE_MIGRATION)? {
        return Ok(());
    }
    sync_code_repository_search_metadata(connection)?;
    connection.execute(
        "
        DELETE FROM code_repository_search
        WHERE rowid IN (
            SELECT search_rowid
            FROM code_repository_search_metadata
            WHERE document_kind = 'call'
        )
        ",
        [],
    )?;
    connection.execute(
        "DELETE FROM code_repository_search_metadata WHERE document_kind = 'call'",
        [],
    )?;
    insert_search_calls(connection)?;
    sync_code_repository_search_metadata(connection)?;
    mark_code_schema_migration(connection, CALL_SEARCH_SIGNATURE_MIGRATION)
}

fn call_search_supports_symbol_signatures(connection: &Connection) -> Result<bool, StorageError> {
    Ok(table_has_columns(
        connection,
        "code_repository_calls",
        &[
            "source_scope",
            "call_id",
            "path",
            "caller_name",
            "callee_name",
            "target_hint",
            "caller_symbol_snapshot_id",
            "callee_symbol_snapshot_id",
        ],
    )? && table_has_columns(
        connection,
        "code_repository_symbols",
        &["source_scope", "symbol_snapshot_id", "signature"],
    )?)
}

fn insert_search_calls(connection: &Connection) -> Result<(), StorageError> {
    if !call_search_supports_symbol_signatures(connection)? {
        connection.execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT call.source_scope, 'call', call.call_id, call.path,
                   coalesce(file.language_id, ''),
                   coalesce(call.caller_name, '') || ' ' || call.callee_name || ' ' ||
                   coalesce(call.target_hint, '') || ' ' || call.path
            FROM code_repository_calls call
            LEFT JOIN code_repository_files file
              ON file.source_scope = call.source_scope
             AND file.path = call.path
            ",
            [],
        )?;

        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT call.source_scope, 'call', call.call_id, call.path,
               coalesce(file.language_id, ''),
               coalesce(call.caller_name, '') || ' ' || call.callee_name || ' ' ||
               coalesce(call.target_hint, '') || ' ' ||
               coalesce(caller.signature, '') || ' ' ||
               coalesce(callee.signature, '') || ' ' || call.path
        FROM code_repository_calls call
        LEFT JOIN code_repository_files file
          ON file.source_scope = call.source_scope
         AND file.path = call.path
        LEFT JOIN code_repository_symbols caller
          ON caller.source_scope = call.source_scope
         AND caller.symbol_snapshot_id = call.caller_symbol_snapshot_id
        LEFT JOIN code_repository_symbols callee
          ON callee.source_scope = call.source_scope
         AND callee.symbol_snapshot_id = call.callee_symbol_snapshot_id
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_chunks(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_chunks",
        &["source_scope", "chunk_id", "path", "language_id", "content"],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'chunk', chunk_id, path, language_id, content
        FROM code_repository_chunks
        ",
        [],
    )?;
    Ok(())
}
