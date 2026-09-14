//! Checkpointed ownership pages for paths excluded from the immutable base clone.

use std::ops::Bound::{Excluded, Unbounded};

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

use super::{
    CloneIdentity, MAX_SOURCE_ROWS_PER_PAGE, PAGE_FIXED_MUTATION_ROWS, clone_capacity_error,
    page_control_bytes, progress, require_page_budget,
};

pub(super) const CURSOR_MARKER: &str = "affected_paths";

pub(super) struct Advance {
    pub(super) completed_steps: usize,
}

pub(super) fn initial_cursor(identity: &CloneIdentity) -> (Option<String>, Option<String>) {
    if identity.affected_paths.is_empty() {
        (None, None)
    } else {
        (Some(String::new()), Some(CURSOR_MARKER.to_owned()))
    }
}

pub(super) fn is_staging(progress: &progress::CloneProgress) -> bool {
    progress.phase == progress::PHASE_TABLES
        && progress.table_ordinal == 0
        && progress.completed_table_ordinal.is_none()
        && progress.expected_table_rows.is_none()
        && progress.cursor_tiebreaker.as_deref() == Some(CURSOR_MARKER)
}

pub(super) fn advance(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    now_ms: u64,
) -> Result<Advance, StorageError> {
    validate_staged_prefix(transaction, current, identity)?;
    let cursor = current.cursor_key.as_deref().ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone affected-path cursor for scope '{}' disappeared",
            current.source_scope
        ))
    })?;
    let row_limit = identity
        .resource_budget
        .max_files_per_batch
        .min(
            current
                .page_row_limit
                .saturating_sub(PAGE_FIXED_MUTATION_ROWS),
        )
        .min(MAX_SOURCE_ROWS_PER_PAGE);
    if row_limit == 0 {
        return Err(clone_capacity_error(&current.source_scope));
    }

    let mut paths = Vec::new();
    let mut path_bytes = 0usize;
    let lower = if current.scanned_table_rows == 0 {
        Unbounded
    } else {
        Excluded(cursor)
    };
    for path in identity.affected_paths.range::<str, _>((lower, Unbounded)) {
        if paths.len() == row_limit {
            break;
        }
        let next_path_bytes = path_bytes
            .checked_add(super::admission::ROW_STORAGE_OVERHEAD_BYTES)
            .and_then(|value| value.checked_add(identity.source_scope.len()))
            .and_then(|value| value.checked_add(path.len()))
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        let mut candidate = current.clone();
        candidate.cursor_key = Some(path.clone());
        candidate.scanned_table_rows = current
            .scanned_table_rows
            .checked_add(paths.len().saturating_add(1))
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        candidate.completed_page_ordinal = current
            .completed_page_ordinal
            .checked_add(1)
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        if identity
            .affected_paths
            .last()
            .is_some_and(|last| last == path)
        {
            candidate.scanned_table_rows = 0;
            candidate.cursor_key = None;
            candidate.cursor_tiebreaker = None;
        }
        let candidate_bytes = page_control_bytes(&candidate, identity)?
            .checked_add(next_path_bytes)
            .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
        if candidate_bytes > current.page_byte_limit {
            if paths.is_empty() {
                return Err(clone_capacity_error(&current.source_scope));
            }
            break;
        }
        path_bytes = next_path_bytes;
        paths.push(path.as_str());
    }
    if paths.is_empty() {
        return Err(StorageError::Invariant(format!(
            "incremental clone affected-path cursor for scope '{}' has no remaining path",
            current.source_scope
        )));
    }

    let last = paths.last().copied().ok_or_else(|| {
        StorageError::Invariant("affected-path page lost its admitted cursor".to_owned())
    })?;
    let complete = identity
        .affected_paths
        .last()
        .is_some_and(|path| path == last);
    let mut insert = transaction.prepare_cached(
        "INSERT INTO code_repository_incremental_clone_affected_paths (source_scope, path)
         VALUES (?1, ?2)",
    )?;
    for path in &paths {
        insert.execute(params![identity.source_scope, path])?;
    }
    drop(insert);

    let mut next = current.clone();
    next.completed_page_ordinal = current
        .completed_page_ordinal
        .checked_add(1)
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    next.scanned_table_rows = current
        .scanned_table_rows
        .checked_add(paths.len())
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    next.cursor_key = Some(last.to_owned());
    if complete {
        next.scanned_table_rows = 0;
        next.cursor_key = None;
        next.cursor_tiebreaker = None;
    }
    require_page_budget(&next, identity, paths.len(), path_bytes, 1, 1)?;
    progress::compare_and_store(transaction, current, &next, now_ms)?;
    Ok(Advance {
        completed_steps: next.completed_page_ordinal,
    })
}

pub(super) fn validate_owner(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
) -> Result<(), StorageError> {
    if is_staging(current) {
        return validate_staged_prefix(transaction, current, identity);
    }
    let (count, first, last) = owner_bounds(transaction, &identity.source_scope)?;
    let expected_first = identity.affected_paths.first().map(String::as_str);
    let expected_last = identity.affected_paths.last().map(String::as_str);
    if count == identity.affected_paths.len()
        && first.as_deref() == expected_first
        && last.as_deref() == expected_last
    {
        return Ok(());
    }
    Err(owner_changed(identity))
}

fn validate_staged_prefix(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
) -> Result<(), StorageError> {
    if !is_staging(current)
        || current.phase != progress::PHASE_TABLES
        || current.table_ordinal != 0
        || current.completed_table_ordinal.is_some()
        || current.expected_table_rows.is_some()
    {
        return Err(owner_changed(identity));
    }
    let cursor = current
        .cursor_key
        .as_deref()
        .ok_or_else(|| owner_changed(identity))?;
    let (count, first, last) = owner_bounds(transaction, &identity.source_scope)?;
    let expected_first = if count > 0 {
        identity.affected_paths.first().map(String::as_str)
    } else {
        None
    };
    let expected_last = (count > 0).then_some(cursor);
    if count == current.scanned_table_rows
        && count < identity.affected_paths.len()
        && first.as_deref() == expected_first
        && last.as_deref() == expected_last
        && (count == 0 || identity.affected_paths.contains(cursor))
    {
        return Ok(());
    }
    Err(owner_changed(identity))
}

fn owner_bounds(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, Option<String>, Option<String>), StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(*), MIN(path), MAX(path)
             FROM code_repository_incremental_clone_affected_paths
             WHERE source_scope = ?1",
            [source_scope],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(StorageError::from)
}

fn owner_changed(identity: &CloneIdentity) -> StorageError {
    StorageError::Invariant(format!(
        "incremental clone affected-path owner for scope '{}' changed",
        identity.source_scope
    ))
}
