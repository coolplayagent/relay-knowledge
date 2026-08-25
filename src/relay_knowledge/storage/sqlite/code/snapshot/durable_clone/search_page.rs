//! Bounded metadata-keyed copy of exact FTS owners.

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::{
    CloneIdentity, clone_capacity_error, progress, require_page_budget, source_row_budget,
};
use crate::storage::sqlite::code::snapshot::admission::ROW_STORAGE_OVERHEAD_BYTES;

#[cfg(test)]
#[path = "search_page_tests.rs"]
mod tests;

struct SearchCursor {
    document_kind: String,
    record_id: String,
}

struct SearchPage {
    row_count: usize,
    affected_count: usize,
    reference_owner_count: usize,
    last: Option<SearchCursor>,
    bytes: usize,
    has_more: bool,
}

pub(super) fn advance(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    now_ms: u64,
) -> Result<(), StorageError> {
    let (row_limit, byte_limit) = source_row_budget(current, identity, 4, 5)?;
    let page = load_page(transaction, current, row_limit, byte_limit)?;
    if page.row_count == 0 {
        return finish_search(transaction, current, identity, now_ms);
    }
    let last = page
        .last
        .as_ref()
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    let copied = super::search_bulk::copy(
        transaction,
        current,
        super::search_bulk::AdmittedRange {
            last_kind: &last.document_kind,
            last_record_id: &last.record_id,
            row_count: page.row_count,
            affected_count: page.affected_count,
        },
    )?;
    let mut next = current.clone();
    next.completed_page_ordinal =
        checked_add(next.completed_page_ordinal, 1, &current.source_scope)?;
    next.scanned_table_rows = checked_add(
        next.scanned_table_rows,
        page.row_count,
        &current.source_scope,
    )?;
    next.copied_table_rows = checked_add(next.copied_table_rows, copied, &current.source_scope)?;
    next.scanned_total_rows = checked_add(
        next.scanned_total_rows,
        page.row_count,
        &current.source_scope,
    )?;
    next.copied_total_rows = checked_add(
        next.copied_total_rows,
        copied.saturating_mul(2),
        &current.source_scope,
    )?;
    next.copied_total_bytes = checked_add(
        next.copied_total_bytes,
        page.bytes.saturating_mul(2),
        &current.source_scope,
    )?;
    next.scanned_reference_search_owner_count = checked_add(
        next.scanned_reference_search_owner_count,
        page.reference_owner_count,
        &current.source_scope,
    )?;
    next.cursor_key = Some(last.document_kind.clone());
    next.cursor_tiebreaker = Some(last.record_id.clone());
    if !page.has_more {
        require_reference_owner_proof(&next)?;
        next.cloned_search_document_count = next.copied_table_rows;
        next.expected_table_rows = Some(next.scanned_table_rows);
        next.completed_table_ordinal = Some(current.table_ordinal);
        next.scanned_table_rows = 0;
        next.copied_table_rows = 0;
        next.phase = progress::PHASE_CLONE_COMPLETE.to_owned();
        next.cursor_key = None;
        next.cursor_tiebreaker = None;
    }
    require_page_budget(&next, identity, page.row_count, page.bytes, 4, 5)?;
    progress::compare_and_store(transaction, current, &next, now_ms)
}

fn finish_search(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    now_ms: u64,
) -> Result<(), StorageError> {
    let mut next = current.clone();
    next.completed_page_ordinal =
        checked_add(next.completed_page_ordinal, 1, &current.source_scope)?;
    require_reference_owner_proof(&next)?;
    next.expected_table_rows = Some(next.scanned_table_rows);
    next.cloned_search_document_count = next.copied_table_rows;
    next.completed_table_ordinal = Some(current.table_ordinal);
    next.scanned_table_rows = 0;
    next.copied_table_rows = 0;
    next.phase = progress::PHASE_CLONE_COMPLETE.to_owned();
    next.cursor_key = None;
    next.cursor_tiebreaker = None;
    require_page_budget(&next, identity, 0, 0, 4, 5)?;
    progress::compare_and_store(transaction, current, &next, now_ms)
}

fn load_page(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    row_limit: usize,
    byte_limit: usize,
) -> Result<SearchPage, StorageError> {
    require_cursor_exists(transaction, current)?;
    let limit = i64::try_from(row_limit.saturating_add(1))
        .map_err(|_| clone_capacity_error(&current.source_scope))?;
    let mut statement = if current.cursor_key.is_some() {
        transaction.prepare(SEARCH_PAGE_AFTER_SQL)?
    } else {
        transaction.prepare(SEARCH_PAGE_FIRST_SQL)?
    };
    let mut rows = match (
        current.cursor_key.as_ref(),
        current.cursor_tiebreaker.as_ref(),
    ) {
        (Some(kind), Some(record_id)) => statement.query(params![
            current.base_scope,
            current.source_scope,
            kind,
            record_id,
            limit,
        ])?,
        (None, None) => {
            statement.query(params![current.base_scope, current.source_scope, limit,])?
        }
        _ => {
            return Err(StorageError::Invariant(
                "incremental clone search cursor is incomplete".to_owned(),
            ));
        }
    };
    let mut row_count = 0usize;
    let mut affected_count = 0usize;
    let mut reference_owner_count = 0usize;
    let mut last_metadata_rowid = None;
    let mut bytes = 0usize;
    let mut has_more = false;
    while let Some(row) = rows.next()? {
        let source_rowid = row.get::<_, Option<i64>>(1)?.ok_or_else(|| {
            StorageError::Invariant(format!(
                "code search scope '{}' has metadata without an exact FTS owner",
                current.base_scope
            ))
        })?;
        let measured = row.get::<_, i64>(2)?;
        let measured = usize::try_from(measured)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, measured))?;
        let row_bytes = measured
            .checked_add(current.source_scope.len().saturating_mul(2))
            .and_then(|value| value.checked_add(ROW_STORAGE_OVERHEAD_BYTES.saturating_mul(2)))
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        let next_bytes = bytes
            .checked_add(row_bytes)
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        if row_count == row_limit || next_bytes > byte_limit {
            if row_count == 0 {
                return Err(clone_capacity_error(&current.source_scope));
            }
            has_more = true;
            break;
        }
        let _ = source_rowid;
        last_metadata_rowid = Some(row.get(0)?);
        affected_count = checked_add(
            affected_count,
            usize::from(row.get::<_, bool>(3)?),
            &current.source_scope,
        )?;
        reference_owner_count = checked_add(
            reference_owner_count,
            usize::from(row.get::<_, bool>(4)?),
            &current.source_scope,
        )?;
        row_count = checked_add(row_count, 1, &current.source_scope)?;
        bytes = next_bytes;
    }
    drop(rows);
    drop(statement);
    let last = last_metadata_rowid
        .map(|rowid| load_cursor(transaction, current, rowid))
        .transpose()?;
    Ok(SearchPage {
        row_count,
        affected_count,
        reference_owner_count,
        last,
        bytes,
        has_more,
    })
}

fn load_cursor(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    metadata_rowid: i64,
) -> Result<SearchCursor, StorageError> {
    transaction
        .query_row(
            "SELECT metadata.document_kind, metadata.record_id
             FROM code_repository_search_metadata metadata
             JOIN code_repository_search search
               ON search.rowid = metadata.search_rowid
              AND search.source_scope = metadata.source_scope
              AND search.document_kind = metadata.document_kind
              AND search.record_id = metadata.record_id
              AND search.path = metadata.path
             WHERE metadata.rowid = ?1 AND metadata.source_scope = ?2",
            params![metadata_rowid, current.base_scope],
            |row| {
                Ok(SearchCursor {
                    document_kind: row.get(0)?,
                    record_id: row.get(1)?,
                })
            },
        )
        .map_err(StorageError::from)
}

fn require_cursor_exists(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
) -> Result<(), StorageError> {
    let (kind, record_id) = match (
        current.cursor_key.as_ref(),
        current.cursor_tiebreaker.as_ref(),
    ) {
        (None, None) => return Ok(()),
        (Some(kind), Some(record_id)) => (kind, record_id),
        _ => {
            return Err(StorageError::Invariant(
                "incremental clone search cursor is incomplete".to_owned(),
            ));
        }
    };
    let exists = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1
             FROM code_repository_search_metadata metadata
             JOIN code_repository_search search
               ON search.rowid = metadata.search_rowid
              AND search.source_scope = metadata.source_scope
              AND search.document_kind = metadata.document_kind
              AND search.record_id = metadata.record_id
              AND search.path = metadata.path
             WHERE metadata.source_scope = ?1
               AND metadata.document_kind = ?2
               AND metadata.record_id = ?3
         )",
        params![current.base_scope, kind, record_id],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone search cursor '{kind}'/'{record_id}' no longer has an exact owner"
    )))
}

fn checked_add(left: usize, right: usize, scope: &str) -> Result<usize, StorageError> {
    left.checked_add(right)
        .ok_or_else(|| clone_capacity_error(scope))
}

fn require_reference_owner_proof(progress: &progress::CloneProgress) -> Result<(), StorageError> {
    if progress.scanned_reference_search_owner_count == progress.base_manifest_group_count {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone search owners for scope '{}' do not match its frozen grouped-reference manifest",
        progress.source_scope
    )))
}

const SEARCH_PAGE_FIRST_SQL: &str = "SELECT metadata.rowid, search.rowid,
            coalesce(length(CAST(metadata.source_scope AS BLOB)), 0)
            + coalesce(length(CAST(metadata.document_kind AS BLOB)), 0)
            + coalesce(length(CAST(metadata.record_id AS BLOB)), 0)
            + coalesce(length(CAST(metadata.path AS BLOB)), 0)
            + coalesce(length(CAST(search.language_id AS BLOB)), 0)
            + coalesce(length(CAST(search.content AS BLOB)), 0),
            EXISTS (
                SELECT 1
                FROM code_repository_incremental_clone_affected_paths affected
                WHERE affected.source_scope = ?2 AND affected.path = metadata.path
            ),
            metadata.document_kind = 'reference'
     FROM code_repository_search_metadata metadata
     LEFT JOIN code_repository_search search
       ON search.rowid = metadata.search_rowid
      AND search.source_scope = metadata.source_scope
      AND search.document_kind = metadata.document_kind
      AND search.record_id = metadata.record_id
      AND search.path = metadata.path
     WHERE metadata.source_scope = ?1
     ORDER BY metadata.document_kind, metadata.record_id
     LIMIT ?3";

const SEARCH_PAGE_AFTER_SQL: &str = "SELECT metadata.rowid, search.rowid,
            coalesce(length(CAST(metadata.source_scope AS BLOB)), 0)
            + coalesce(length(CAST(metadata.document_kind AS BLOB)), 0)
            + coalesce(length(CAST(metadata.record_id AS BLOB)), 0)
            + coalesce(length(CAST(metadata.path AS BLOB)), 0)
            + coalesce(length(CAST(search.language_id AS BLOB)), 0)
            + coalesce(length(CAST(search.content AS BLOB)), 0),
            EXISTS (
                SELECT 1
                FROM code_repository_incremental_clone_affected_paths affected
                WHERE affected.source_scope = ?2 AND affected.path = metadata.path
            ),
            metadata.document_kind = 'reference'
     FROM code_repository_search_metadata metadata
     LEFT JOIN code_repository_search search
       ON search.rowid = metadata.search_rowid
      AND search.source_scope = metadata.source_scope
      AND search.document_kind = metadata.document_kind
      AND search.record_id = metadata.record_id
      AND search.path = metadata.path
     WHERE metadata.source_scope = ?1
       AND (metadata.document_kind, metadata.record_id) > (?3, ?4)
     ORDER BY metadata.document_kind, metadata.record_id
     LIMIT ?5";
