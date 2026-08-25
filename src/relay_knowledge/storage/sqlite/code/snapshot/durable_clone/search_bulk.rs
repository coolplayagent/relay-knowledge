//! Bulk copy for one byte-admitted metadata key range.

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::{clone_capacity_error, progress};

pub(super) struct AdmittedRange<'a> {
    pub(super) last_kind: &'a str,
    pub(super) last_record_id: &'a str,
    pub(super) row_count: usize,
    pub(super) affected_count: usize,
}

pub(super) fn copy(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    range: AdmittedRange<'_>,
) -> Result<usize, StorageError> {
    require_canonical_range(transaction, current, &range)?;
    let expected = range
        .row_count
        .checked_sub(range.affected_count)
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    super::super::super::search::require_consecutive_search_rowids(transaction)?;
    let copied = if let (Some(kind), Some(record_id)) = (
        current.cursor_key.as_deref(),
        current.cursor_tiebreaker.as_deref(),
    ) {
        transaction.execute(
            COPY_AFTER_SQL,
            params![
                current.base_scope,
                current.source_scope,
                kind,
                record_id,
                range.last_kind,
                range.last_record_id,
            ],
        )?
    } else {
        transaction.execute(
            COPY_FIRST_SQL,
            params![
                current.base_scope,
                current.source_scope,
                range.last_kind,
                range.last_record_id,
            ],
        )?
    };
    if copied != expected {
        return Err(StorageError::Invariant(format!(
            "incremental clone search range copied {copied} owners from {expected} admitted owners"
        )));
    }
    if copied == 0 {
        return Ok(0);
    }
    let last_rowid = transaction.last_insert_rowid();
    let copied_i64 =
        i64::try_from(copied).map_err(|_| clone_capacity_error(&current.source_scope))?;
    let first_rowid = last_rowid
        .checked_sub(copied_i64.saturating_sub(1))
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    let metadata = transaction.execute(
        "INSERT INTO code_repository_search_metadata (
             source_scope, document_kind, record_id, path, search_rowid
         )
         SELECT source_scope, document_kind, record_id, path, rowid
         FROM code_repository_search
         WHERE rowid BETWEEN ?1 AND ?2 AND source_scope = ?3",
        params![first_rowid, last_rowid, current.source_scope],
    )?;
    if metadata == copied {
        return Ok(copied);
    }
    Err(StorageError::Invariant(format!(
        "incremental clone search range persisted {copied} FTS rows but {metadata} metadata owners"
    )))
}

fn require_canonical_range(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    range: &AdmittedRange<'_>,
) -> Result<(), StorageError> {
    let exact = if let (Some(kind), Some(record_id)) = (
        current.cursor_key.as_deref(),
        current.cursor_tiebreaker.as_deref(),
    ) {
        transaction.query_row(
            CANONICAL_AFTER_SQL,
            params![
                current.base_scope,
                kind,
                record_id,
                range.last_kind,
                range.last_record_id,
            ],
            |row| row.get::<_, usize>(0),
        )?
    } else {
        transaction.query_row(
            CANONICAL_FIRST_SQL,
            params![current.base_scope, range.last_kind, range.last_record_id,],
            |row| row.get::<_, usize>(0),
        )?
    };
    if exact == range.row_count {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "code search scope '{}' has a noncanonical owner in its admitted key range",
        current.base_scope
    )))
}

const CANONICAL_FIRST_SQL: &str = "SELECT count(*)
     FROM code_repository_search_metadata metadata
     JOIN code_repository_search search
       ON search.rowid = metadata.search_rowid
      AND search.source_scope = metadata.source_scope
      AND search.document_kind = metadata.document_kind
      AND search.record_id = metadata.record_id
      AND search.path = metadata.path
     WHERE metadata.source_scope = ?1
       AND (metadata.document_kind, metadata.record_id) <= (?2, ?3)
       AND (metadata.document_kind <> 'reference' OR EXISTS (
           SELECT 1 FROM code_repository_reference_search_groups reference_group
           WHERE reference_group.source_scope = metadata.source_scope
             AND reference_group.group_id = metadata.record_id
             AND reference_group.path = metadata.path
             AND reference_group.language_id = search.language_id
             AND trim(reference_group.name) <> '' AND trim(reference_group.kind) <> ''
             AND trim(reference_group.path) <> ''
             AND search.content = reference_group.name || ' ' || reference_group.kind
                 || CASE WHEN trim(reference_group.target_hint) = '' THEN ''
                         ELSE ' ' || reference_group.target_hint END
                 || ' ' || reference_group.path
       ))";

const CANONICAL_AFTER_SQL: &str = "SELECT count(*)
     FROM code_repository_search_metadata metadata
     JOIN code_repository_search search
       ON search.rowid = metadata.search_rowid
      AND search.source_scope = metadata.source_scope
      AND search.document_kind = metadata.document_kind
      AND search.record_id = metadata.record_id
      AND search.path = metadata.path
     WHERE metadata.source_scope = ?1
       AND (metadata.document_kind, metadata.record_id) > (?2, ?3)
       AND (metadata.document_kind, metadata.record_id) <= (?4, ?5)
       AND (metadata.document_kind <> 'reference' OR EXISTS (
           SELECT 1 FROM code_repository_reference_search_groups reference_group
           WHERE reference_group.source_scope = metadata.source_scope
             AND reference_group.group_id = metadata.record_id
             AND reference_group.path = metadata.path
             AND reference_group.language_id = search.language_id
             AND trim(reference_group.name) <> '' AND trim(reference_group.kind) <> ''
             AND trim(reference_group.path) <> ''
             AND search.content = reference_group.name || ' ' || reference_group.kind
                 || CASE WHEN trim(reference_group.target_hint) = '' THEN ''
                         ELSE ' ' || reference_group.target_hint END
                 || ' ' || reference_group.path
       ))";

const COPY_FIRST_SQL: &str = "INSERT INTO code_repository_search (
         source_scope, document_kind, record_id, path, language_id, content
     )
     SELECT ?2, metadata.document_kind, metadata.record_id, metadata.path,
            source.language_id, source.content
     FROM code_repository_search_metadata metadata
     JOIN code_repository_search source
       ON source.rowid = metadata.search_rowid
      AND source.source_scope = metadata.source_scope
      AND source.document_kind = metadata.document_kind
      AND source.record_id = metadata.record_id
      AND source.path = metadata.path
     WHERE metadata.source_scope = ?1
       AND (metadata.document_kind, metadata.record_id) <= (?3, ?4)
       AND NOT EXISTS (
           SELECT 1 FROM code_repository_incremental_clone_affected_paths affected
           WHERE affected.source_scope = ?2 AND affected.path = metadata.path
       )
     ORDER BY metadata.document_kind, metadata.record_id";

const COPY_AFTER_SQL: &str = "INSERT INTO code_repository_search (
         source_scope, document_kind, record_id, path, language_id, content
     )
     SELECT ?2, metadata.document_kind, metadata.record_id, metadata.path,
            source.language_id, source.content
     FROM code_repository_search_metadata metadata
     JOIN code_repository_search source
       ON source.rowid = metadata.search_rowid
      AND source.source_scope = metadata.source_scope
      AND source.document_kind = metadata.document_kind
      AND source.record_id = metadata.record_id
      AND source.path = metadata.path
     WHERE metadata.source_scope = ?1
       AND (metadata.document_kind, metadata.record_id) > (?3, ?4)
       AND (metadata.document_kind, metadata.record_id) <= (?5, ?6)
       AND NOT EXISTS (
           SELECT 1 FROM code_repository_incremental_clone_affected_paths affected
           WHERE affected.source_scope = ?2 AND affected.path = metadata.path
       )
     ORDER BY metadata.document_kind, metadata.record_id";
