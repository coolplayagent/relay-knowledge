use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::snapshot_import::IMPORT_SCHEMA;

pub(super) fn clone_exact_search_documents(
    transaction: &Transaction<'_>,
    base_scope: &str,
    target_scope: &str,
    expected_count: usize,
) -> Result<(), StorageError> {
    copy_search_documents(
        transaction,
        target_scope,
        expected_count,
        "INSERT INTO code_repository_search (
             source_scope, document_kind, record_id, path, language_id, content
         )
         SELECT ?2, source_search.document_kind, source_search.record_id,
                source_search.path, source_search.language_id, source_search.content
         FROM code_repository_search_metadata source_owner
         CROSS JOIN code_repository_search source_search
           ON source_search.rowid = source_owner.search_rowid
          AND source_search.source_scope = source_owner.source_scope
          AND source_search.document_kind = source_owner.document_kind
          AND source_search.record_id = source_owner.record_id
          AND source_search.path = source_owner.path
         WHERE source_owner.source_scope = ?1
         ORDER BY source_owner.search_rowid",
        params![base_scope, target_scope],
    )
}

pub(super) fn import_exact_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let (metadata_count, owned_count) = transaction.query_row(
        &format!(
            "
            SELECT
                (SELECT COUNT(*)
                 FROM {IMPORT_SCHEMA}.code_repository_search_metadata
                 WHERE source_scope = ?1),
                (SELECT COUNT(*)
                 FROM {IMPORT_SCHEMA}.code_repository_search_metadata source_owner
                 CROSS JOIN {IMPORT_SCHEMA}.code_repository_search source_search
                   ON source_search.rowid = source_owner.search_rowid
                  AND source_search.source_scope = source_owner.source_scope
                  AND source_search.document_kind = source_owner.document_kind
                  AND source_search.record_id = source_owner.record_id
                  AND source_search.path = source_owner.path
                 WHERE source_owner.source_scope = ?1)
            "
        ),
        params![source_scope],
        |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
    )?;
    require_exact_source_ownership(source_scope, metadata_count, owned_count)?;
    copy_search_documents(
        transaction,
        source_scope,
        metadata_count,
        &format!(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT source_search.source_scope, source_search.document_kind,
                   source_search.record_id, source_search.path,
                   source_search.language_id, source_search.content
            FROM {IMPORT_SCHEMA}.code_repository_search_metadata source_owner
            CROSS JOIN {IMPORT_SCHEMA}.code_repository_search source_search
              ON source_search.rowid = source_owner.search_rowid
             AND source_search.source_scope = source_owner.source_scope
             AND source_search.document_kind = source_owner.document_kind
             AND source_search.record_id = source_owner.record_id
             AND source_search.path = source_owner.path
            WHERE source_owner.source_scope = ?1
            ORDER BY source_owner.search_rowid
            "
        ),
        params![source_scope],
    )
}

fn require_exact_source_ownership(
    source_scope: &str,
    metadata_count: usize,
    owned_count: usize,
) -> Result<(), StorageError> {
    if metadata_count != owned_count {
        return Err(StorageError::Invariant(format!(
            "code search scope '{source_scope}' does not have exact FTS metadata ownership: \
             metadata={metadata_count}, exact_owned={owned_count}"
        )));
    }

    Ok(())
}

fn copy_search_documents<P>(
    transaction: &Transaction<'_>,
    target_scope: &str,
    expected_count: usize,
    copy_sql: &str,
    copy_params: P,
) -> Result<(), StorageError>
where
    P: rusqlite::Params,
{
    let previous_rowid = transaction.query_row(
        "SELECT coalesce(max(rowid), 0) FROM code_repository_search",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let copied_count = transaction.execute(copy_sql, copy_params)?;
    let last_rowid = transaction.query_row(
        "SELECT coalesce(max(rowid), 0) FROM code_repository_search",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let bounded_count = transaction.query_row(
        "
        SELECT COUNT(*)
        FROM code_repository_search
        WHERE rowid > ?1
          AND rowid <= ?2
        ",
        params![previous_rowid, last_rowid],
        |row| row.get::<_, usize>(0),
    )?;
    if copied_count != expected_count || bounded_count != copied_count {
        return Err(StorageError::Invariant(format!(
            "code search copy for scope '{target_scope}' changed an unexpected row count: \
             expected={expected_count}, copied={copied_count}, bounded={bounded_count}"
        )));
    }
    let metadata_count = transaction.execute(
        "
        INSERT INTO code_repository_search_metadata (
            source_scope, document_kind, record_id, path, search_rowid
        )
        SELECT source_scope, document_kind, record_id, path, rowid
        FROM code_repository_search
        WHERE rowid > ?1
          AND rowid <= ?2
        ",
        params![previous_rowid, last_rowid],
    )?;
    if metadata_count != copied_count {
        return Err(StorageError::Invariant(format!(
            "code search copy for scope '{target_scope}' did not establish one metadata owner \
             per copied FTS row: copied={copied_count}, metadata={metadata_count}"
        )));
    }

    Ok(())
}

#[cfg(test)]
#[path = "search_copy_tests.rs"]
mod tests;
