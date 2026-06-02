use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::super::super::{SearchDocumentInserter, code_search::delete_search_documents_for_kind};

pub(super) fn rebuild_reference_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    delete_search_documents_for_kind(transaction, source_scope, "reference")?;
    let mut select = transaction.prepare(
        "
        SELECT reference.reference_id, reference.path, coalesce(file.language_id, ''),
               reference.name, reference.kind, coalesce(reference.target_hint, '')
        FROM code_repository_references reference
        LEFT JOIN code_repository_files file
          ON file.source_scope = reference.source_scope
         AND file.path = reference.path
        WHERE reference.source_scope = ?1
        ",
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let name = row.get::<_, String>(3)?;
        let kind = row.get::<_, String>(4)?;
        let target_hint = row.get::<_, String>(5)?;
        inserter.insert(
            source_scope,
            "reference",
            &record_id,
            &path,
            &language_id,
            [
                name.as_str(),
                kind.as_str(),
                target_hint.as_str(),
                path.as_str(),
            ],
        )?;
    }

    Ok(())
}

pub(super) fn rebuild_import_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    delete_search_documents_for_kind(transaction, source_scope, "import")?;
    let mut select = transaction.prepare(
        "
        SELECT import.import_id, import.path, coalesce(file.language_id, ''),
               import.module, coalesce(import.target_hint, '')
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        WHERE import.source_scope = ?1
        ",
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let module = row.get::<_, String>(3)?;
        let target_hint = row.get::<_, String>(4)?;
        inserter.insert(
            source_scope,
            "import",
            &record_id,
            &path,
            &language_id,
            [module.as_str(), target_hint.as_str(), path.as_str()],
        )?;
    }

    Ok(())
}

pub(super) fn rebuild_call_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    delete_search_documents_for_kind(transaction, source_scope, "call")?;
    let mut select = transaction.prepare(
        "
        SELECT call.call_id, call.path, coalesce(file.language_id, ''),
               coalesce(call.caller_name, ''), call.callee_name,
               coalesce(call.target_hint, ''), coalesce(caller.signature, ''),
               coalesce(callee.signature, '')
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
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let caller_name = row.get::<_, String>(3)?;
        let callee_name = row.get::<_, String>(4)?;
        let target_hint = row.get::<_, String>(5)?;
        let caller_signature = row.get::<_, String>(6)?;
        let callee_signature = row.get::<_, String>(7)?;
        inserter.insert(
            source_scope,
            "call",
            &record_id,
            &path,
            &language_id,
            [
                caller_name.as_str(),
                callee_name.as_str(),
                target_hint.as_str(),
                caller_signature.as_str(),
                callee_signature.as_str(),
                path.as_str(),
            ],
        )?;
    }

    Ok(())
}
