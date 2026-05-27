use rusqlite::{Transaction, params};

use crate::storage::StorageError;

pub(super) fn rebuild_reference_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE source_scope = ?1 AND document_kind = 'reference'
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT reference.source_scope, 'reference', reference.reference_id, reference.path,
               coalesce(file.language_id, ''),
               reference.name || ' ' || reference.kind || ' ' ||
               coalesce(reference.target_hint, '') || ' ' || reference.path
        FROM code_repository_references reference
        LEFT JOIN code_repository_files file
          ON file.source_scope = reference.source_scope
         AND file.path = reference.path
        WHERE reference.source_scope = ?1
        ",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn rebuild_import_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE source_scope = ?1 AND document_kind = 'import'
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT import.source_scope, 'import', import.import_id, import.path,
               coalesce(file.language_id, ''),
               trim(
                   import.module ||
                   CASE
                       WHEN coalesce(import.target_hint, '') = '' THEN ''
                       ELSE ' ' || import.target_hint
                   END ||
                   ' ' || import.path
               )
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        WHERE import.source_scope = ?1
        ",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn rebuild_call_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE source_scope = ?1 AND document_kind = 'call'
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT call.source_scope, 'call', call.call_id, call.path,
               coalesce(file.language_id, ''),
               trim(
                   coalesce(call.caller_name, '') || ' ' ||
                   call.callee_name || ' ' ||
                   coalesce(call.target_hint, '') || ' ' ||
                   coalesce(caller.signature, '') || ' ' ||
                   coalesce(callee.signature, '') || ' ' ||
                   call.path
               )
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
        WHERE call.source_scope = ?1
        ",
        params![source_scope],
    )?;

    Ok(())
}
