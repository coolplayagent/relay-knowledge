//! Bounded primary-key pages for durable incremental base facts.

use rusqlite::{Transaction, params_from_iter, types::Value};

use crate::storage::StorageError;

use super::{
    CloneIdentity, clone_capacity_error, progress, require_page_budget, source_row_budget,
    table_at, table_count,
};
use crate::storage::sqlite::code::snapshot::{
    admission::{ROW_STORAGE_OVERHEAD_BYTES, validated_identifier_length_sql},
    scope_tables::{CodeScopeCursor, CodeScopeTable},
};

struct Candidate {
    key: String,
    tiebreaker: Option<String>,
}

struct CandidatePage {
    row_count: usize,
    affected_count: usize,
    reference_occurrences: usize,
    last: Option<Candidate>,
    bytes: usize,
    has_more: bool,
}

pub(super) fn advance(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    now_ms: u64,
) -> Result<(), StorageError> {
    let table = table_at(current.table_ordinal).ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone table ordinal {} is outside its plan",
            current.table_ordinal
        ))
    })?;
    let (row_limit, byte_limit) = source_row_budget(current, identity, 2, 3)?;
    let page = load_page(transaction, table, current, row_limit, byte_limit)?;
    if page.row_count == 0 {
        return finish_table(transaction, current, table, identity, now_ms);
    }
    let last = page
        .last
        .as_ref()
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    let expected_copies = page
        .row_count
        .checked_sub(page.affected_count)
        .ok_or_else(|| clone_capacity_error(&current.source_scope))?;
    let copied = copy_prefix(transaction, table, current, last)?;
    if copied != expected_copies {
        return Err(StorageError::Invariant(format!(
            "incremental clone table '{}' copied {copied} rows from a prefix owning {expected_copies}",
            table.table
        )));
    }

    let eof = !page.has_more;
    let mut next = current.clone();
    next.completed_page_ordinal =
        checked_add(next.completed_page_ordinal, 1, &current.source_scope)?;
    next.copied_table_rows = checked_add(next.copied_table_rows, copied, &current.source_scope)?;
    next.scanned_table_rows = checked_add(
        next.scanned_table_rows,
        page.row_count,
        &current.source_scope,
    )?;
    next.scanned_total_rows = checked_add(
        next.scanned_total_rows,
        page.row_count,
        &current.source_scope,
    )?;
    next.copied_total_rows = checked_add(next.copied_total_rows, copied, &current.source_scope)?;
    next.copied_total_bytes = checked_add(
        next.copied_total_bytes,
        page.bytes.saturating_mul(2),
        &current.source_scope,
    )?;
    next.scanned_reference_occurrence_count = checked_add(
        next.scanned_reference_occurrence_count,
        page.reference_occurrences,
        &current.source_scope,
    )?;
    match table.table {
        "code_repository_references" => {
            next.scanned_reference_row_count = checked_add(
                next.scanned_reference_row_count,
                page.row_count,
                &current.source_scope,
            )?;
        }
        "code_repository_reference_search_groups" => {
            next.scanned_reference_group_count = checked_add(
                next.scanned_reference_group_count,
                page.row_count,
                &current.source_scope,
            )?;
        }
        _ => {}
    }
    next.cursor_key = Some(last.key.clone());
    next.cursor_tiebreaker = last.tiebreaker.clone();
    if eof {
        require_completed_table_proof(&next, table.table)?;
        freeze_table_counter(&mut next, table.table)?;
        next.expected_table_rows = Some(next.scanned_table_rows);
        next.completed_table_ordinal = Some(current.table_ordinal);
        next.scanned_table_rows = 0;
        next.copied_table_rows = 0;
        next.table_ordinal = checked_add(next.table_ordinal, 1, &current.source_scope)?;
        next.cursor_key = None;
        next.cursor_tiebreaker = None;
        if next.table_ordinal == table_count() {
            next.phase = progress::PHASE_SEARCH.to_owned();
        }
    }
    require_page_budget(&next, identity, page.row_count, page.bytes, 2, 3)?;
    progress::compare_and_store(transaction, current, &next, now_ms)
}

fn finish_table(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    table: &CodeScopeTable,
    identity: &CloneIdentity,
    now_ms: u64,
) -> Result<(), StorageError> {
    let mut next = current.clone();
    next.completed_page_ordinal =
        checked_add(next.completed_page_ordinal, 1, &current.source_scope)?;
    require_completed_table_proof(&next, table.table)?;
    freeze_table_counter(&mut next, table.table)?;
    next.expected_table_rows = Some(next.scanned_table_rows);
    next.completed_table_ordinal = Some(current.table_ordinal);
    next.scanned_table_rows = 0;
    next.copied_table_rows = 0;
    next.table_ordinal = checked_add(next.table_ordinal, 1, &current.source_scope)?;
    next.cursor_key = None;
    next.cursor_tiebreaker = None;
    if next.table_ordinal == table_count() {
        next.phase = progress::PHASE_SEARCH.to_owned();
    }
    require_page_budget(&next, identity, 0, 0, 2, 3)?;
    progress::compare_and_store(transaction, current, &next, now_ms)
}

fn load_page(
    transaction: &Transaction<'_>,
    table: &CodeScopeTable,
    current: &progress::CloneProgress,
    row_limit: usize,
    byte_limit: usize,
) -> Result<CandidatePage, StorageError> {
    require_cursor_exists(transaction, table, current)?;
    let length_sql = table
        .columns
        .split(',')
        .map(str::trim)
        .map(validated_identifier_length_sql)
        .collect::<Result<Vec<_>, _>>()?
        .join(" + ");
    let limit = i64::try_from(row_limit.saturating_add(1))
        .map_err(|_| clone_capacity_error(&current.source_scope))?;
    let (sql, values) = candidate_query(table, current, &length_sql, limit)?;
    let mut statement = transaction.prepare(&sql)?;
    let mut query = statement.query(params_from_iter(values))?;
    let mut row_count = 0usize;
    let mut affected_count = 0usize;
    let mut reference_occurrences = 0usize;
    let mut last_rowid = None;
    let mut bytes = 0usize;
    let mut has_more = false;
    while let Some(row) = query.next()? {
        let measured = row.get::<_, i64>(1)?;
        let measured = usize::try_from(measured)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, measured))?;
        let row_bytes = measured
            .checked_add(current.source_scope.len())
            .and_then(|value| value.checked_add(ROW_STORAGE_OVERHEAD_BYTES))
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
        last_rowid = Some(row.get(0)?);
        affected_count = checked_add(
            affected_count,
            usize::from(row.get::<_, bool>(2)?),
            &current.source_scope,
        )?;
        reference_occurrences = checked_add(
            reference_occurrences,
            usize::try_from(row.get::<_, i64>(3)?)
                .map_err(|_| clone_capacity_error(&current.source_scope))?,
            &current.source_scope,
        )?;
        row_count = checked_add(row_count, 1, &current.source_scope)?;
        bytes = next_bytes;
    }
    drop(query);
    drop(statement);
    let last = last_rowid
        .map(|rowid| load_cursor(transaction, table, current, rowid))
        .transpose()?;
    Ok(CandidatePage {
        row_count,
        affected_count,
        reference_occurrences,
        last,
        bytes,
        has_more,
    })
}

fn load_cursor(
    transaction: &Transaction<'_>,
    table: &CodeScopeTable,
    current: &progress::CloneProgress,
    rowid: i64,
) -> Result<Candidate, StorageError> {
    let (key, tiebreaker) = match table.cursor {
        CodeScopeCursor::Key(key) => transaction.query_row(
            &format!(
                "SELECT \"{key}\", NULL FROM {table_name}
                 WHERE rowid = ?1 AND source_scope = ?2",
                table_name = table.table,
            ),
            rusqlite::params![rowid, current.base_scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
        CodeScopeCursor::Pair(key, tiebreaker) => transaction.query_row(
            &format!(
                "SELECT \"{key}\", \"{tiebreaker}\" FROM {table_name}
                 WHERE rowid = ?1 AND source_scope = ?2",
                table_name = table.table,
            ),
            rusqlite::params![rowid, current.base_scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
        CodeScopeCursor::Singleton => {
            return Err(StorageError::Invariant(format!(
                "incremental clone table '{}' cannot load a singleton cursor",
                table.table
            )));
        }
    };
    Ok(Candidate { key, tiebreaker })
}

fn require_cursor_exists(
    transaction: &Transaction<'_>,
    table: &CodeScopeTable,
    current: &progress::CloneProgress,
) -> Result<(), StorageError> {
    let exists = match (
        table.cursor,
        current.cursor_key.as_ref(),
        current.cursor_tiebreaker.as_ref(),
    ) {
        (CodeScopeCursor::Key(_), None, None) | (CodeScopeCursor::Pair(_, _), None, None) => {
            return Ok(());
        }
        (CodeScopeCursor::Key(key), Some(cursor), None) => transaction.query_row(
            &format!(
                "SELECT EXISTS (
                     SELECT 1 FROM {table_name}
                     WHERE source_scope = ?1 AND \"{key}\" = ?2
                 )",
                table_name = table.table,
            ),
            rusqlite::params![current.base_scope, cursor],
            |row| row.get::<_, bool>(0),
        )?,
        (CodeScopeCursor::Pair(key, tiebreaker), Some(cursor), Some(cursor_tiebreaker)) => {
            transaction.query_row(
                &format!(
                    "SELECT EXISTS (
                         SELECT 1 FROM {table_name}
                         WHERE source_scope = ?1 AND \"{key}\" = ?2
                           AND \"{tiebreaker}\" = ?3
                     )",
                    table_name = table.table,
                ),
                rusqlite::params![current.base_scope, cursor, cursor_tiebreaker],
                |row| row.get::<_, bool>(0),
            )?
        }
        (CodeScopeCursor::Singleton, _, _) => {
            return Err(StorageError::Invariant(format!(
                "incremental clone table '{}' has no keyset cursor",
                table.table
            )));
        }
        _ => {
            return Err(StorageError::Invariant(format!(
                "incremental clone cursor for table '{}' has an invalid shape",
                table.table
            )));
        }
    };
    if exists {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone cursor for table '{}' no longer exists in base scope '{}'",
        table.table, current.base_scope
    )))
}

fn candidate_query(
    table: &CodeScopeTable,
    current: &progress::CloneProgress,
    length_sql: &str,
    limit: i64,
) -> Result<(String, Vec<Value>), StorageError> {
    let mut values = vec![
        Value::Text(current.base_scope.clone()),
        Value::Text(current.source_scope.clone()),
    ];
    let (after, order) = match table.cursor {
        CodeScopeCursor::Key(key) => {
            let after = if let Some(cursor) = current.cursor_key.as_ref() {
                values.push(Value::Text(cursor.clone()));
                format!("AND source.\"{key}\" > ?3")
            } else {
                String::new()
            };
            (after, format!("source.\"{key}\""))
        }
        CodeScopeCursor::Pair(key, tiebreaker) => {
            let after = match (
                current.cursor_key.as_ref(),
                current.cursor_tiebreaker.as_ref(),
            ) {
                (Some(cursor), Some(cursor_tiebreaker)) => {
                    values.push(Value::Text(cursor.clone()));
                    values.push(Value::Text(cursor_tiebreaker.clone()));
                    format!("AND (source.\"{key}\", source.\"{tiebreaker}\") > (?3, ?4)")
                }
                (None, None) => String::new(),
                _ => {
                    return Err(StorageError::Invariant(
                        "incremental clone pair cursor is incomplete".to_owned(),
                    ));
                }
            };
            (after, format!("source.\"{key}\", source.\"{tiebreaker}\""))
        }
        CodeScopeCursor::Singleton => {
            return Err(StorageError::Invariant(format!(
                "incremental clone table '{}' cannot use a singleton cursor",
                table.table
            )));
        }
    };
    values.push(Value::Integer(limit));
    let limit_parameter = values.len();
    Ok((
        format!(
            "SELECT source.rowid, ({length_sql}),
                    EXISTS (
                        SELECT 1
                        FROM code_repository_incremental_clone_affected_paths affected
                        WHERE affected.source_scope = ?2
                          AND affected.path = source.path
                    ),
                    {reference_occurrences}
             FROM {table_name} source
             WHERE source.source_scope = ?1 {after}
             ORDER BY {order}
             LIMIT ?{limit_parameter}",
            table_name = table.table,
            reference_occurrences = if table.table == "code_repository_reference_search_groups" {
                "source.occurrence_count"
            } else {
                "0"
            },
        ),
        values,
    ))
}

fn copy_prefix(
    transaction: &Transaction<'_>,
    table: &CodeScopeTable,
    current: &progress::CloneProgress,
    last: &Candidate,
) -> Result<usize, StorageError> {
    let selected_columns = table.columns.replacen("source_scope", "?2", 1);
    let mut values = vec![
        Value::Text(current.base_scope.clone()),
        Value::Text(current.source_scope.clone()),
    ];
    let range = match table.cursor {
        CodeScopeCursor::Key(key) => {
            values.push(Value::Text(last.key.clone()));
            let lower = if let Some(cursor) = current.cursor_key.as_ref() {
                values.push(Value::Text(cursor.clone()));
                format!("AND source.\"{key}\" > ?4")
            } else {
                String::new()
            };
            format!("AND source.\"{key}\" <= ?3 {lower}")
        }
        CodeScopeCursor::Pair(key, tiebreaker) => {
            let last_tiebreaker = last.tiebreaker.as_ref().ok_or_else(|| {
                StorageError::Invariant("incremental clone pair endpoint is incomplete".to_owned())
            })?;
            values.push(Value::Text(last.key.clone()));
            values.push(Value::Text(last_tiebreaker.clone()));
            let lower = match (
                current.cursor_key.as_ref(),
                current.cursor_tiebreaker.as_ref(),
            ) {
                (Some(cursor), Some(cursor_tiebreaker)) => {
                    values.push(Value::Text(cursor.clone()));
                    values.push(Value::Text(cursor_tiebreaker.clone()));
                    format!("AND (source.\"{key}\", source.\"{tiebreaker}\") > (?5, ?6)")
                }
                (None, None) => String::new(),
                _ => {
                    return Err(StorageError::Invariant(
                        "incremental clone pair cursor is incomplete".to_owned(),
                    ));
                }
            };
            format!("AND (source.\"{key}\", source.\"{tiebreaker}\") <= (?3, ?4) {lower}")
        }
        CodeScopeCursor::Singleton => unreachable!("singleton tables are excluded from clone plan"),
    };
    transaction
        .execute(
            &format!(
                "INSERT INTO {table_name} ({columns})
                 SELECT {selected_columns}
                 FROM {table_name} source
                 WHERE source.source_scope = ?1 {range}
                   AND NOT EXISTS (
                       SELECT 1
                       FROM code_repository_incremental_clone_affected_paths affected
                       WHERE affected.source_scope = ?2
                         AND affected.path = source.path
                   )",
                table_name = table.table,
                columns = table.columns,
            ),
            params_from_iter(values),
        )
        .map_err(StorageError::from)
}

fn freeze_table_counter(
    next: &mut progress::CloneProgress,
    table: &str,
) -> Result<(), StorageError> {
    match table {
        "code_repository_files" => next.cloned_file_count = next.copied_table_rows,
        "code_repository_symbols" => next.cloned_symbol_count = next.copied_table_rows,
        "code_repository_references" => next.cloned_reference_count = next.copied_table_rows,
        "code_repository_chunks" => next.cloned_chunk_count = next.copied_table_rows,
        "code_repository_file_diagnostics" => next.cloned_diagnostic_count = next.copied_table_rows,
        "code_repository_reference_search_groups" => {
            next.cloned_reference_group_count = next.copied_table_rows
        }
        _ => {}
    }
    Ok(())
}

fn require_completed_table_proof(
    progress: &progress::CloneProgress,
    table: &str,
) -> Result<(), StorageError> {
    let exact = match table {
        "code_repository_references" => {
            progress.scanned_table_rows == progress.base_manifest_reference_count
                && progress.scanned_reference_row_count == progress.base_manifest_reference_count
        }
        "code_repository_reference_search_groups" => {
            progress.scanned_table_rows == progress.base_manifest_group_count
                && progress.scanned_reference_group_count == progress.base_manifest_group_count
                && progress.scanned_reference_occurrence_count
                    == progress.base_manifest_reference_count
        }
        _ => true,
    };
    if exact {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone table '{table}' does not match its frozen grouped-reference manifest"
    )))
}

fn checked_add(left: usize, right: usize, scope: &str) -> Result<usize, StorageError> {
    left.checked_add(right)
        .ok_or_else(|| clone_capacity_error(scope))
}
