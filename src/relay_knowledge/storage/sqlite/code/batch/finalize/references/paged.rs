//! Advances ordinary-reference resolution one durable keyset page at a time.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeReferenceResolution, CodeReferenceResolutionStage,
        code_reference_resolution_cursor_digest, code_reference_resolution_state,
    },
    storage::StorageError,
};

use super::paged_sql;
use crate::storage::sqlite::code::batch::finalize::pages::{
    FINALIZATION_PAGE_BYTE_HARD_LIMIT, FinalizationPageLimits, TextPagePlan, checkpoint_row_bytes,
    require_admitted_page, require_quantum_bytes,
};

const PROTOCOL_VERSION: usize = 1;
const OWNER: &str = "reference-resolution";
// Six integer payloads/serial types, three worst-case text serial types, and
// the SQLite record-header length varint.
const PROGRESS_RECORD_FIXED_BYTES: usize = 6 * 9 + 3 * 9 + 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::storage::sqlite::code::batch) enum ReferenceResolutionAdvance {
    Pending {
        completed_page_ordinal: usize,
        completed_reference_count: usize,
        cursor_reference_id: Option<String>,
    },
    Complete,
}

struct Progress {
    protocol_version: usize,
    stage: String,
    completed_page_ordinal: usize,
    cursor_reference_id: Option<String>,
    expected_reference_count: usize,
    resolved_reference_count: usize,
    limits: FinalizationPageLimits,
}

#[derive(Clone, Copy)]
enum SymbolOwners {
    Missing,
    Unique(usize),
    Ambiguous,
}

pub(in crate::storage::sqlite::code::batch) fn initialize(
    transaction: &Transaction<'_>,
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<ReferenceResolutionAdvance, StorageError> {
    let existing = transaction
        .query_row(
            "SELECT 1 FROM code_repository_reference_resolution_progress
             WHERE source_scope = ?1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?;
    if existing.is_some() {
        return Err(StorageError::Invariant(format!(
            "reference-resolution progress for scope '{source_scope}' already exists before initialization"
        )));
    }
    if expected_reference_count == 0 {
        let facts_exist = transaction.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_references
                 WHERE source_scope = ?1 LIMIT 1
             )",
            params![source_scope],
            |row| row.get::<_, bool>(0),
        )?;
        if facts_exist {
            return Err(StorageError::Invariant(format!(
                "reference-resolution zero count for scope '{source_scope}' does not match its facts"
            )));
        }
        let checkpoint_bytes = checkpoint_row_bytes(
            transaction,
            source_scope,
            super::super::phases::RESOLVE_REFERENCES,
        )?;
        require_quantum_bytes(
            source_scope,
            OWNER,
            resource_budget
                .max_bytes_per_batch
                .min(FINALIZATION_PAGE_BYTE_HARD_LIMIT),
            checkpoint_bytes,
        )?;
        return Ok(ReferenceResolutionAdvance::Complete);
    }
    let limits = FinalizationPageLimits::derive(source_scope, OWNER, resource_budget, 1)?;
    let initial_state = code_reference_resolution_state(0, 0, None).ok_or_else(|| {
        StorageError::Invariant(
            "initial reference-resolution checkpoint token is not canonical".to_owned(),
        )
    })?;
    let initial_control_bytes = checkpoint_row_bytes(transaction, source_scope, &initial_state)?
        .checked_add(progress_row_bytes(source_scope, None)?)
        .ok_or_else(|| control_overflow(source_scope))?;
    require_quantum_bytes(
        source_scope,
        OWNER,
        limits.byte_limit,
        initial_control_bytes,
    )?;
    let inserted = transaction.execute(
        "INSERT INTO code_repository_reference_resolution_progress (
             source_scope, protocol_version, stage, completed_page_ordinal,
             cursor_reference_id, expected_reference_count, resolved_reference_count,
             page_document_limit, page_byte_limit
         ) VALUES (?1, ?2, 'resolve', 0, NULL, ?3, 0, ?4, ?5)",
        params![
            source_scope,
            PROTOCOL_VERSION,
            expected_reference_count,
            limits.document_limit,
            limits.byte_limit,
        ],
    )?;
    require_single_progress_mutation(source_scope, inserted)?;
    Ok(ReferenceResolutionAdvance::Pending {
        completed_page_ordinal: 0,
        completed_reference_count: 0,
        cursor_reference_id: None,
    })
}

pub(in crate::storage::sqlite::code::batch) fn advance(
    transaction: &Transaction<'_>,
    source_scope: &str,
    checkpoint: CodeReferenceResolution,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<ReferenceResolutionAdvance, StorageError> {
    let progress = load_progress(transaction, source_scope)?;
    let expected_limits = FinalizationPageLimits::derive(source_scope, OWNER, resource_budget, 1)?;
    if progress.limits != expected_limits {
        return Err(StorageError::Invariant(format!(
            "reference-resolution progress for scope '{source_scope}' does not match its durable resource budget"
        )));
    }
    if progress.expected_reference_count != expected_reference_count {
        return Err(StorageError::Invariant(format!(
            "reference-resolution progress for scope '{source_scope}' does not match its frozen checkpoint reference count"
        )));
    }
    require_progress_matches_checkpoint(transaction, source_scope, &progress, checkpoint)?;
    let plan = page_plan(transaction, source_scope, &progress)?;
    require_admitted_page(source_scope, OWNER, progress.limits, &plan)?;
    let Some(last_cursor) = plan.last_cursor.as_deref() else {
        if progress.resolved_reference_count != progress.expected_reference_count {
            return Err(progress_count_error(source_scope));
        }
        require_eof_cursor_at_scope_tail(transaction, source_scope, &progress)?;
        let eof_control_bytes = checkpoint_row_bytes(
            transaction,
            source_scope,
            super::super::phases::RESOLVE_REFERENCES,
        )?
        .checked_add(progress_row_bytes(
            source_scope,
            progress.cursor_reference_id.as_deref(),
        )?)
        .ok_or_else(|| control_overflow(source_scope))?;
        require_quantum_bytes(
            source_scope,
            OWNER,
            progress.limits.byte_limit,
            eof_control_bytes,
        )?;
        let deleted = transaction.execute(
            "DELETE FROM code_repository_reference_resolution_progress
             WHERE source_scope = ?1 AND protocol_version = ?2 AND stage = 'resolve'
               AND completed_page_ordinal = ?3
               AND cursor_reference_id IS ?4
               AND expected_reference_count = ?5
               AND resolved_reference_count = ?6",
            params![
                source_scope,
                progress.protocol_version,
                progress.completed_page_ordinal,
                progress.cursor_reference_id,
                progress.expected_reference_count,
                progress.resolved_reference_count,
            ],
        )?;
        require_single_progress_mutation(source_scope, deleted)?;
        return Ok(ReferenceResolutionAdvance::Complete);
    };

    let updated = match progress.cursor_reference_id.as_deref() {
        Some(cursor) => transaction.execute(
            paged_sql::UPDATE_AFTER,
            params![source_scope, cursor, last_cursor],
        )?,
        None => transaction.execute(paged_sql::UPDATE_FIRST, params![source_scope, last_cursor])?,
    };
    if updated != plan.mutation_count {
        return Err(StorageError::Invariant(format!(
            "reference-resolution page for scope '{source_scope}' did not update its exact planned range"
        )));
    }
    let next_page = progress
        .completed_page_ordinal
        .checked_add(1)
        .ok_or_else(|| page_overflow(source_scope, "page ordinal"))?;
    let next_resolved = progress
        .resolved_reference_count
        .checked_add(plan.row_count)
        .ok_or_else(|| page_overflow(source_scope, "resolved-reference count"))?;
    if next_resolved > progress.expected_reference_count {
        return Err(progress_count_error(source_scope));
    }
    let changed = transaction.execute(
        "UPDATE code_repository_reference_resolution_progress
         SET completed_page_ordinal = ?3, cursor_reference_id = ?4,
             resolved_reference_count = ?5
         WHERE source_scope = ?1 AND protocol_version = ?6 AND stage = 'resolve'
           AND completed_page_ordinal = ?2
           AND cursor_reference_id IS ?7
           AND expected_reference_count = ?8
           AND resolved_reference_count = ?9",
        params![
            source_scope,
            progress.completed_page_ordinal,
            next_page,
            last_cursor,
            next_resolved,
            progress.protocol_version,
            progress.cursor_reference_id,
            progress.expected_reference_count,
            progress.resolved_reference_count,
        ],
    )?;
    require_single_progress_mutation(source_scope, changed)?;
    Ok(ReferenceResolutionAdvance::Pending {
        completed_page_ordinal: next_page,
        completed_reference_count: next_resolved,
        cursor_reference_id: Some(last_cursor.to_owned()),
    })
}

fn load_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Progress, StorageError> {
    transaction
        .query_row(
            "SELECT protocol_version, stage, completed_page_ordinal,
                    cursor_reference_id, expected_reference_count,
                    resolved_reference_count, page_document_limit, page_byte_limit
             FROM code_repository_reference_resolution_progress
             WHERE source_scope = ?1",
            params![source_scope],
            |row| {
                let document_limit = row.get(6)?;
                let byte_limit = row.get(7)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    document_limit,
                    byte_limit,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "reference-resolution progress for scope '{source_scope}' is unavailable"
            ))
        })
        .and_then(
            |(
                protocol_version,
                stage,
                completed_page_ordinal,
                cursor_reference_id,
                expected_reference_count,
                resolved_reference_count,
                document_limit,
                byte_limit,
            )| {
                Ok(Progress {
                    protocol_version,
                    stage,
                    completed_page_ordinal,
                    cursor_reference_id,
                    expected_reference_count,
                    resolved_reference_count,
                    limits: FinalizationPageLimits::from_persisted(
                        source_scope,
                        OWNER,
                        document_limit,
                        byte_limit,
                    )?,
                })
            },
        )
}

fn require_progress_matches_checkpoint(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
    checkpoint: CodeReferenceResolution,
) -> Result<(), StorageError> {
    let maximum_resolved_count = progress
        .completed_page_ordinal
        .checked_mul(progress.limits.document_limit)
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "reference-resolution progress for scope '{source_scope}' has an overflowing page/count bound"
            ))
        })?;
    let cursor_exists = match progress.cursor_reference_id.as_deref() {
        Some(cursor) => transaction.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_references
                 WHERE source_scope = ?1 AND reference_id = ?2
             )",
            params![source_scope, cursor],
            |row| row.get::<_, bool>(0),
        )?,
        None => false,
    };
    if progress.protocol_version != PROTOCOL_VERSION
        || progress.stage != "resolve"
        || checkpoint.protocol_version
            != u32::try_from(PROTOCOL_VERSION).expect("protocol version should fit u32")
        || checkpoint.stage != CodeReferenceResolutionStage::Resolve
        || progress.completed_page_ordinal != checkpoint.completed_page_ordinal
        || progress.resolved_reference_count != checkpoint.completed_reference_count
        || code_reference_resolution_cursor_digest(progress.cursor_reference_id.as_deref())
            != checkpoint.cursor_digest
        || progress.resolved_reference_count > progress.expected_reference_count
        || (progress.completed_page_ordinal == 0
            && (progress.cursor_reference_id.is_some() || progress.resolved_reference_count != 0))
        || (progress.completed_page_ordinal > 0 && progress.cursor_reference_id.is_none())
        || (progress.completed_page_ordinal > 0 && progress.resolved_reference_count == 0)
        || progress.completed_page_ordinal > progress.resolved_reference_count
        || progress.resolved_reference_count > maximum_resolved_count
        || (progress.cursor_reference_id.is_some() && !cursor_exists)
    {
        return Err(StorageError::Invariant(format!(
            "reference-resolution progress for scope '{source_scope}' does not match its durable checkpoint"
        )));
    }
    Ok(())
}

fn require_eof_cursor_at_scope_tail(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<(), StorageError> {
    let tail = transaction
        .query_row(
            "SELECT reference_id FROM code_repository_references
             WHERE source_scope = ?1 ORDER BY reference_id DESC LIMIT 1",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if tail.as_deref() != progress.cursor_reference_id.as_deref() {
        return Err(StorageError::Invariant(format!(
            "reference-resolution cursor for scope '{source_scope}' is not the exact fact tail at EOF"
        )));
    }
    Ok(())
}

fn page_plan(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<TextPagePlan, StorageError> {
    let control_bytes = page_control_bytes(transaction, source_scope)?;
    let mut statement = match progress.cursor_reference_id.as_deref() {
        Some(_) => transaction.prepare(paged_sql::SCAN_AFTER)?,
        None => transaction.prepare(paged_sql::SCAN_FIRST)?,
    };
    let mut rows = match progress.cursor_reference_id.as_deref() {
        Some(cursor) => statement.query(params![
            source_scope,
            cursor,
            progress.limits.document_limit
        ])?,
        None => statement.query(params![source_scope, progress.limits.document_limit])?,
    };
    let mut name_owners = HashMap::<String, SymbolOwners>::new();
    let mut path_owners = HashMap::<(String, String), SymbolOwners>::new();
    let mut row_count = 0usize;
    let mut mutation_count = 0usize;
    let mut owner_bytes = 0usize;
    let mut first_row_bytes = None;
    let mut last_candidate_rowid = None;
    while let Some(row) = rows.next()? {
        let candidate_rowid = row.get::<_, i64>(0)?;
        let cursor_bytes = row.get::<_, usize>(1)?;
        let is_call = row.get::<_, bool>(2)?;
        let base_owner_bytes = row.get::<_, usize>(3)?;
        if is_call {
            let quantum_bytes = control_bytes
                .checked_add(owner_bytes)
                .and_then(|bytes| bytes.checked_add(cursor_bytes))
                .ok_or_else(|| control_overflow(source_scope))?;
            first_row_bytes.get_or_insert(quantum_bytes);
            if quantum_bytes > progress.limits.byte_limit {
                break;
            }
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| page_overflow(source_scope, "page row count"))?;
            last_candidate_rowid = Some(candidate_rowid);
            continue;
        }
        let minimum_decision_bytes = "resolved".len() + "inferred".len();
        let minimum_quantum = control_bytes
            .checked_add(owner_bytes)
            .and_then(|bytes| bytes.checked_add(base_owner_bytes))
            .and_then(|bytes| bytes.checked_add(cursor_bytes))
            .and_then(|bytes| bytes.checked_add(minimum_decision_bytes))
            .ok_or_else(|| control_overflow(source_scope))?;
        if minimum_quantum > progress.limits.byte_limit {
            first_row_bytes.get_or_insert(minimum_quantum);
            break;
        }
        let (_, path, name) = transaction.query_row(
            paged_sql::FETCH_CANDIDATE,
            params![source_scope, candidate_rowid],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let decision_bytes = resolution_decision_bytes(
            transaction,
            source_scope,
            &name,
            &path,
            &mut name_owners,
            &mut path_owners,
        )?;
        let next_owner_bytes = base_owner_bytes
            .checked_add(decision_bytes)
            .ok_or_else(|| control_overflow(source_scope))?;
        let quantum_bytes = control_bytes
            .checked_add(owner_bytes)
            .and_then(|bytes| bytes.checked_add(next_owner_bytes))
            .and_then(|bytes| bytes.checked_add(cursor_bytes))
            .ok_or_else(|| control_overflow(source_scope))?;
        first_row_bytes.get_or_insert(quantum_bytes);
        if quantum_bytes > progress.limits.byte_limit {
            break;
        }
        owner_bytes = owner_bytes
            .checked_add(next_owner_bytes)
            .ok_or_else(|| control_overflow(source_scope))?;
        row_count = row_count
            .checked_add(1)
            .ok_or_else(|| page_overflow(source_scope, "page row count"))?;
        mutation_count = mutation_count
            .checked_add(1)
            .ok_or_else(|| page_overflow(source_scope, "page mutation count"))?;
        last_candidate_rowid = Some(candidate_rowid);
    }
    let last_cursor = last_candidate_rowid
        .map(|candidate_rowid| {
            transaction.query_row(
                paged_sql::FETCH_CURSOR,
                params![source_scope, candidate_rowid],
                |row| row.get::<_, String>(0),
            )
        })
        .transpose()?;
    Ok(TextPagePlan {
        row_count,
        mutation_count,
        last_cursor,
        first_row_bytes,
    })
}

fn resolution_decision_bytes(
    transaction: &Transaction<'_>,
    source_scope: &str,
    name: &str,
    path: &str,
    name_cache: &mut HashMap<String, SymbolOwners>,
    path_cache: &mut HashMap<(String, String), SymbolOwners>,
) -> Result<usize, StorageError> {
    let name_owners = cached_symbol_owners(
        transaction,
        paged_sql::NAME_OWNERS,
        params![source_scope, name],
        name_cache,
        name.to_owned(),
    )?;
    if let SymbolOwners::Unique(owner_bytes) = name_owners {
        return Ok(owner_bytes + "resolved".len() + "inferred".len());
    }
    if matches!(name_owners, SymbolOwners::Missing) {
        return Ok("unresolved".len() + "ambiguous".len());
    }
    let pair = (name.to_owned(), path.to_owned());
    let path_owners = cached_symbol_owners(
        transaction,
        paged_sql::PATH_OWNERS,
        params![source_scope, name, path],
        path_cache,
        pair,
    )?;
    if let SymbolOwners::Unique(owner_bytes) = path_owners {
        return Ok(owner_bytes + "resolved".len() + "inferred".len());
    }
    Ok("ambiguous".len() * 2)
}

fn cached_symbol_owners<K, P>(
    transaction: &Transaction<'_>,
    sql: &str,
    parameters: P,
    cache: &mut HashMap<K, SymbolOwners>,
    key: K,
) -> Result<SymbolOwners, StorageError>
where
    K: std::hash::Hash + Eq + Clone,
    P: rusqlite::Params,
{
    if let Some(owners) = cache.get(&key) {
        return Ok(*owners);
    }
    let mut statement = transaction.prepare_cached(sql)?;
    let owners = statement
        .query_map(parameters, |row| row.get::<_, usize>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let summary = match owners.as_slice() {
        [] => SymbolOwners::Missing,
        [owner] => SymbolOwners::Unique(*owner),
        [_, _, ..] => SymbolOwners::Ambiguous,
    };
    cache.insert(key, summary);
    Ok(summary)
}

pub(super) fn page_control_bytes(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<usize, StorageError> {
    let maximum_state = code_reference_resolution_state(
        usize::MAX,
        usize::MAX,
        Some("maximum-cursor-digest-shape"),
    )
    .ok_or_else(|| {
        StorageError::Invariant(
            "maximum reference-resolution checkpoint token is not canonical".to_owned(),
        )
    })?;
    checkpoint_row_bytes(transaction, source_scope, &maximum_state)?
        .checked_add(progress_row_bytes(source_scope, None)?)
        .ok_or_else(|| control_overflow(source_scope))
}

fn progress_row_bytes(
    source_scope: &str,
    cursor_reference_id: Option<&str>,
) -> Result<usize, StorageError> {
    source_scope
        .len()
        .checked_add("resolve".len())
        .and_then(|bytes| bytes.checked_add(cursor_reference_id.map_or(0, str::len)))
        .and_then(|bytes| bytes.checked_add(PROGRESS_RECORD_FIXED_BYTES))
        .ok_or_else(|| control_overflow(source_scope))
}

fn control_overflow(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "reference-resolution control bytes for scope '{source_scope}' exceed platform capacity"
    ))
}

fn require_single_progress_mutation(
    source_scope: &str,
    changed: usize,
) -> Result<(), StorageError> {
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "reference-resolution progress for scope '{source_scope}' did not mutate exactly one row"
        )));
    }
    Ok(())
}

fn progress_count_error(source_scope: &str) -> StorageError {
    StorageError::Invariant(format!(
        "reference-resolution progress counts for scope '{source_scope}' are inconsistent"
    ))
}

fn page_overflow(source_scope: &str, field: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "reference-resolution {field} for scope '{source_scope}' exceeds platform capacity"
    ))
}
