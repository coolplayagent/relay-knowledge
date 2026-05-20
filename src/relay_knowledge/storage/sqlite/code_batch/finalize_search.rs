use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::super::super::SearchDocumentInserter;

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

pub(super) struct CallSearchDocument<'a> {
    pub(super) source_scope: &'a str,
    pub(super) record_id: &'a str,
    pub(super) path: &'a str,
    pub(super) language_id: &'a str,
    pub(super) caller_name: Option<&'a str>,
    pub(super) callee_name: &'a str,
    pub(super) target_hint: Option<&'a str>,
    pub(super) caller_signature: Option<&'a str>,
    pub(super) callee_signature: Option<&'a str>,
}

pub(super) fn insert_call_search_document(
    search_documents: &mut SearchDocumentInserter<'_>,
    document: CallSearchDocument<'_>,
) -> Result<(), StorageError> {
    search_documents.insert(
        document.source_scope,
        "call",
        document.record_id,
        document.path,
        document.language_id,
        [
            document.caller_name.unwrap_or_default(),
            document.callee_name,
            document.target_hint.unwrap_or_default(),
            document.caller_signature.unwrap_or_default(),
            document.callee_signature.unwrap_or_default(),
            document.path,
        ],
    )
}
