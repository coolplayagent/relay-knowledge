//! Builds one durable FTS owner per finalized reference-search equivalence group.

use rusqlite::{OptionalExtension, ToSql, Transaction, params, params_from_iter};

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeReferenceSearchRebuild, CodeReferenceSearchRebuildStage,
        code_reference_search_rebuild_state,
    },
    storage::StorageError,
};

use super::super::super::super::search::require_consecutive_search_rowids;
use super::super::pages::{checkpoint_row_bytes, require_quantum_bytes};

#[path = "grouped_sql.rs"]
mod sql;

#[cfg(test)]
#[path = "grouped_plan_tests.rs"]
mod grouped_plan_tests;

pub(super) const REFERENCE_SEARCH_PROJECTION_VERSION: usize = 2;
const PAGE_DOCUMENT_HARD_LIMIT: usize = 32_768;
const PAGE_BYTE_HARD_LIMIT: usize = 16 * 1024 * 1024;
// Twelve integer payloads/serial types, five worst-case text serial types,
// and the SQLite record-header length varint.
const PROGRESS_RECORD_NON_TEXT_BYTES: usize = 12 * 9 + 5 * 9 + 9;
// One text plus three integers in the manifest owner record.
const MANIFEST_RECORD_NON_TEXT_BYTES: usize = 4 * 9 + 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::storage::sqlite::code::batch) enum ReferenceSearchAdvance {
    Pending {
        stage: CodeReferenceSearchRebuildStage,
        completed_page_ordinal: usize,
    },
    Complete,
}

struct Progress {
    projection_version: usize,
    stage: CodeReferenceSearchRebuildStage,
    completed_page_ordinal: usize,
    cleanup_cursor_rowid: Option<i64>,
    cleanup_cursor_record_id: Option<String>,
    discovery_cursor_reference_id: Option<String>,
    build_cursor_group_id: Option<String>,
    expected_reference_count: usize,
    cleanup_total_count: usize,
    discovered_reference_count: usize,
    discovered_group_count: usize,
    build_total_count: usize,
    cleaned_count: usize,
    built_count: usize,
    page_document_limit: usize,
    page_byte_limit: usize,
}

struct PagePlan<T> {
    row_count: usize,
    last_cursor: Option<T>,
    first_row_bytes: Option<usize>,
}

pub(in crate::storage::sqlite::code::batch) fn initialize_reference_search_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<ReferenceSearchAdvance, StorageError> {
    let existing = transaction
        .query_row(
            "SELECT 1 FROM code_repository_reference_search_progress WHERE source_scope = ?1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?;
    if existing.is_some() {
        return Err(StorageError::Invariant(format!(
            "reference-search progress for scope '{source_scope}' already exists before initialization"
        )));
    }
    let (page_document_limit, page_byte_limit) =
        reference_search_page_limits(source_scope, resource_budget)?;
    let initial_control_bytes = grouped_checkpoint_bytes(transaction, source_scope)?
        .checked_add(initial_progress_row_bytes(source_scope))
        .ok_or_else(|| progress_byte_overflow(source_scope))?;
    require_quantum_bytes(
        source_scope,
        "reference-search",
        page_byte_limit,
        initial_control_bytes,
    )?;
    transaction.execute(
        "INSERT INTO code_repository_reference_search_progress (
             source_scope, projection_version, stage, completed_page_ordinal, cleanup_cursor_rowid,
             cleanup_cursor_record_id,
             discovery_cursor_reference_id, build_cursor_group_id,
             expected_reference_count, cleanup_total_count, discovered_reference_count,
             discovered_group_count, build_total_count, cleaned_count, built_count,
             page_document_limit, page_byte_limit
         ) VALUES (?1, ?2, 'cleanup', 0, NULL, NULL, NULL, NULL, ?3, 0, 0, 0, 0, 0, 0, ?4, ?5)",
        params![
            source_scope,
            REFERENCE_SEARCH_PROJECTION_VERSION,
            expected_reference_count,
            page_document_limit,
            page_byte_limit,
        ],
    )?;
    Ok(pending(CodeReferenceSearchRebuildStage::Cleanup, 0))
}

pub(in crate::storage::sqlite::code::batch) fn advance_reference_search_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
    checkpoint: CodeReferenceSearchRebuild,
) -> Result<ReferenceSearchAdvance, StorageError> {
    let progress = load_progress(transaction, source_scope)?;
    if progress.projection_version == 1 {
        return super::legacy::restart_legacy_progress(
            transaction,
            source_scope,
            progress.stage,
            progress.completed_page_ordinal,
            progress.expected_reference_count,
            checkpoint,
        );
    }
    let expected_limits = reference_search_page_limits(
        source_scope,
        durable_resource_budget(transaction, source_scope)?,
    )?;
    if (progress.page_document_limit, progress.page_byte_limit) != expected_limits {
        return Err(StorageError::Invariant(format!(
            "reference-search progress for scope '{source_scope}' does not match its durable resource budget"
        )));
    }
    require_progress_matches_checkpoint(source_scope, &progress, checkpoint)?;
    match progress.stage {
        CodeReferenceSearchRebuildStage::Cleanup => {
            advance_cleanup(transaction, source_scope, progress)
        }
        CodeReferenceSearchRebuildStage::Discover => {
            advance_discovery(transaction, source_scope, progress)
        }
        CodeReferenceSearchRebuildStage::Build => {
            advance_build(transaction, source_scope, progress)
        }
    }
}

pub(super) fn reference_search_page_limits(
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
) -> Result<(usize, usize), StorageError> {
    // Cleanup can delete one group owner, one FTS row, and one metadata row per document.
    // Reserve both durable control mutations: progress CAS plus checkpoint CAS.
    let page_document_limit = resource_budget
        .max_rows_per_batch
        .saturating_sub(2)
        .saturating_div(3)
        .min(PAGE_DOCUMENT_HARD_LIMIT);
    if page_document_limit == 0 || resource_budget.max_bytes_per_batch == 0 {
        return Err(StorageError::CapacityExceeded(format!(
            "reference-search pages for scope '{source_scope}' require at least five bounded SQLite rows and one byte"
        )));
    }
    Ok((
        page_document_limit,
        resource_budget
            .max_bytes_per_batch
            .min(PAGE_BYTE_HARD_LIMIT),
    ))
}

fn advance_cleanup(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: Progress,
) -> Result<ReferenceSearchAdvance, StorageError> {
    let plan = cleanup_page_plan(transaction, source_scope, &progress)?;
    require_first_row_within_byte_limit(source_scope, "cleanup", &progress, &plan)?;
    let Some(last_cursor) = plan.last_cursor else {
        require_cleanup_complete(transaction, source_scope, &progress)?;
        let transition_bytes = grouped_checkpoint_bytes(transaction, source_scope)?
            .checked_add(progress_row_bytes_without_active_cursor(
                source_scope,
                &progress,
            ))
            .and_then(|bytes| manifest_row_bytes(source_scope).ok()?.checked_add(bytes))
            .ok_or_else(|| progress_byte_overflow(source_scope))?;
        require_quantum_bytes(
            source_scope,
            "reference-search",
            progress.page_byte_limit,
            transition_bytes,
        )?;
        transaction.execute(
            "DELETE FROM code_repository_reference_search_manifests WHERE source_scope = ?1",
            params![source_scope],
        )?;
        let changed = transaction.execute(
            "UPDATE code_repository_reference_search_progress
             SET stage = 'discover', completed_page_ordinal = 0,
                 cleanup_cursor_rowid = NULL, cleanup_cursor_record_id = NULL
             WHERE source_scope = ?1 AND stage = 'cleanup'
               AND completed_page_ordinal = ?2 AND cleaned_count = cleanup_total_count",
            params![source_scope, progress.completed_page_ordinal],
        )?;
        require_single_progress_update(source_scope, changed)?;
        return Ok(pending(CodeReferenceSearchRebuildStage::Discover, 0));
    };
    let deleted_groups = match progress.cleanup_cursor_record_id.as_deref() {
        Some(cursor) => transaction.execute(
            sql::CLEANUP_GROUPS_AFTER,
            params![source_scope, cursor, last_cursor],
        )?,
        None => transaction.execute(
            sql::CLEANUP_GROUPS_FIRST,
            params![source_scope, last_cursor],
        )?,
    };
    if deleted_groups > plan.row_count {
        return Err(progress_count_error(source_scope, "cleanup"));
    }
    let (deleted_search, deleted_metadata) = match progress.cleanup_cursor_record_id.as_deref() {
        Some(cursor) => (
            transaction.execute(
                sql::CLEANUP_SEARCH_AFTER,
                params![source_scope, cursor, last_cursor],
            )?,
            transaction.execute(
                sql::CLEANUP_METADATA_AFTER,
                params![source_scope, cursor, last_cursor],
            )?,
        ),
        None => (
            transaction.execute(
                sql::CLEANUP_SEARCH_FIRST,
                params![source_scope, last_cursor],
            )?,
            transaction.execute(
                sql::CLEANUP_METADATA_FIRST,
                params![source_scope, last_cursor],
            )?,
        ),
    };
    if deleted_search != plan.row_count || deleted_metadata != plan.row_count {
        return Err(StorageError::Invariant(format!(
            "reference-search cleanup for scope '{source_scope}' did not delete its exact bounded page"
        )));
    }
    let next_page = checked_add(progress.completed_page_ordinal, 1, "cleanup page ordinal")?;
    let next_cleaned = checked_add(progress.cleaned_count, plan.row_count, "cleaned-row count")?;
    let next_cleanup_total = checked_add(
        progress.cleanup_total_count,
        plan.row_count,
        "cleanup total-row count",
    )?;
    if next_cleanup_total > progress.expected_reference_count {
        return Err(progress_count_error(source_scope, "cleanup"));
    }
    let changed = transaction.execute(
        "UPDATE code_repository_reference_search_progress
         SET completed_page_ordinal = ?3, cleanup_cursor_record_id = ?4,
             cleanup_total_count = ?5, cleaned_count = ?6
         WHERE source_scope = ?1 AND stage = 'cleanup'
           AND completed_page_ordinal = ?2 AND cleanup_total_count = ?7
           AND cleaned_count = ?8 AND cleanup_cursor_record_id IS ?9",
        params![
            source_scope,
            progress.completed_page_ordinal,
            next_page,
            last_cursor,
            next_cleanup_total,
            next_cleaned,
            progress.cleanup_total_count,
            progress.cleaned_count,
            progress.cleanup_cursor_record_id,
        ],
    )?;
    require_single_progress_update(source_scope, changed)?;
    Ok(pending(CodeReferenceSearchRebuildStage::Cleanup, next_page))
}

fn require_cleanup_complete(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<(), StorageError> {
    if progress.cleaned_count != progress.cleanup_total_count {
        return Err(progress_count_error(source_scope, "cleanup"));
    }
    // Do not scan the raw FTS virtual table in reverse. Metadata is the indexed owner surface;
    // unowned raw rows are never served and the bounded retention orphan phase owns their cleanup.
    let (metadata_remains, groups_remain) = transaction.query_row(
        "SELECT
             EXISTS (SELECT 1 FROM code_repository_search_metadata
                     WHERE source_scope = ?1 AND document_kind = 'reference' LIMIT 1),
             EXISTS (SELECT 1 FROM code_repository_reference_search_groups
                     WHERE source_scope = ?1 LIMIT 1)",
        params![source_scope],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
    )?;
    if metadata_remains || groups_remain {
        return Err(StorageError::Invariant(format!(
            "reference-search cleanup for scope '{source_scope}' left an unowned search or group row"
        )));
    }
    Ok(())
}

fn advance_discovery(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: Progress,
) -> Result<ReferenceSearchAdvance, StorageError> {
    let plan = discovery_page_plan(transaction, source_scope, &progress)?;
    require_first_row_within_byte_limit(source_scope, "discovery", &progress, &plan)?;
    let Some(last_cursor) = plan.last_cursor.as_deref() else {
        require_discovery_complete(source_scope, &progress)?;
        let changed = transaction.execute(
            "UPDATE code_repository_reference_search_progress
             SET stage = 'build', completed_page_ordinal = 0,
                 discovery_cursor_reference_id = NULL, build_total_count = ?3
             WHERE source_scope = ?1 AND stage = 'discover'
               AND completed_page_ordinal = ?2
               AND discovered_reference_count = expected_reference_count",
            params![
                source_scope,
                progress.completed_page_ordinal,
                progress.discovered_group_count,
            ],
        )?;
        require_single_progress_update(source_scope, changed)?;
        return Ok(pending(CodeReferenceSearchRebuildStage::Build, 0));
    };
    let (page_group_count, new_group_count) =
        upsert_discovery_page(transaction, source_scope, &progress, last_cursor)?;
    if page_group_count == 0 {
        return Err(StorageError::Invariant(format!(
            "reference-search discovery for scope '{source_scope}' did not upsert its exact bounded input page"
        )));
    }
    let next_page = checked_add(progress.completed_page_ordinal, 1, "discovery page ordinal")?;
    let next_discovered = checked_add(
        progress.discovered_reference_count,
        plan.row_count,
        "discovered-reference count",
    )?;
    if next_discovered > progress.expected_reference_count {
        return Err(progress_count_error(source_scope, "discovery"));
    }
    let next_group_count = checked_add(
        progress.discovered_group_count,
        new_group_count,
        "discovered-group count",
    )?;
    let changed = transaction.execute(
        "UPDATE code_repository_reference_search_progress
         SET completed_page_ordinal = ?3, discovery_cursor_reference_id = ?4,
             discovered_reference_count = ?5, discovered_group_count = ?6
         WHERE source_scope = ?1 AND stage = 'discover'
           AND completed_page_ordinal = ?2 AND discovered_reference_count = ?7
           AND discovered_group_count = ?8 AND discovery_cursor_reference_id IS ?9",
        params![
            source_scope,
            progress.completed_page_ordinal,
            next_page,
            last_cursor,
            next_discovered,
            next_group_count,
            progress.discovered_reference_count,
            progress.discovered_group_count,
            progress.discovery_cursor_reference_id,
        ],
    )?;
    require_single_progress_update(source_scope, changed)?;
    Ok(pending(
        CodeReferenceSearchRebuildStage::Discover,
        next_page,
    ))
}

fn require_discovery_complete(source_scope: &str, progress: &Progress) -> Result<(), StorageError> {
    if progress.discovered_reference_count != progress.expected_reference_count {
        return Err(progress_count_error(source_scope, "discovery"));
    }
    Ok(())
}

fn upsert_discovery_page(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
    planned_last_cursor: &str,
) -> Result<(usize, usize), StorageError> {
    let previous_cursor = progress.discovery_cursor_reference_id.as_deref();
    let (statement_sql, values): (&str, Vec<&dyn ToSql>) = match previous_cursor {
        Some(_) => (
            sql::DISCOVERY_UPSERT_AFTER,
            vec![
                &source_scope,
                &progress.discovery_cursor_reference_id,
                &planned_last_cursor,
            ],
        ),
        None => (
            sql::DISCOVERY_UPSERT_FIRST,
            vec![&source_scope, &planned_last_cursor],
        ),
    };
    let mut statement = transaction.prepare(statement_sql)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut page_group_count = 0usize;
    let mut new_group_count = 0usize;
    while let Some(row) = rows.next()? {
        let group_id = row.get::<_, String>(0)?;
        page_group_count = checked_add(page_group_count, 1, "page group count")?;
        if previous_cursor.is_none_or(|cursor| group_id.as_str() > cursor) {
            new_group_count = checked_add(new_group_count, 1, "new group count")?;
        }
    }
    Ok((page_group_count, new_group_count))
}

fn advance_build(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: Progress,
) -> Result<ReferenceSearchAdvance, StorageError> {
    let plan = build_page_plan(transaction, source_scope, &progress)?;
    require_first_row_within_byte_limit(source_scope, "build", &progress, &plan)?;
    let Some(last_cursor) = plan.last_cursor.as_deref() else {
        require_build_complete(source_scope, &progress)?;
        let transition_bytes = grouped_checkpoint_bytes(transaction, source_scope)?
            .checked_add(progress_row_bytes_without_active_cursor(
                source_scope,
                &progress,
            ))
            .and_then(|bytes| manifest_row_bytes(source_scope).ok()?.checked_add(bytes))
            .ok_or_else(|| progress_byte_overflow(source_scope))?;
        require_quantum_bytes(
            source_scope,
            "reference-search",
            progress.page_byte_limit,
            transition_bytes,
        )?;
        transaction.execute(
            "INSERT INTO code_repository_reference_search_manifests (
                 source_scope, projection_version, reference_count, group_count
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                source_scope,
                REFERENCE_SEARCH_PROJECTION_VERSION,
                progress.expected_reference_count,
                progress.build_total_count,
            ],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM code_repository_reference_search_progress
             WHERE source_scope = ?1 AND stage = 'build'
               AND completed_page_ordinal = ?2 AND built_count = build_total_count",
            params![source_scope, progress.completed_page_ordinal],
        )?;
        require_single_progress_update(source_scope, deleted)?;
        return Ok(ReferenceSearchAdvance::Complete);
    };
    let inserted_search =
        insert_group_search_page(transaction, source_scope, &progress, last_cursor)?;
    if inserted_search != plan.row_count {
        return Err(StorageError::Invariant(format!(
            "reference-search build for scope '{source_scope}' did not insert its exact bounded FTS page"
        )));
    }
    let next_page = checked_add(progress.completed_page_ordinal, 1, "build page ordinal")?;
    let next_built = checked_add(progress.built_count, plan.row_count, "built-row count")?;
    if next_built > progress.build_total_count {
        return Err(progress_count_error(source_scope, "build"));
    }
    let changed = transaction.execute(
        "UPDATE code_repository_reference_search_progress
         SET completed_page_ordinal = ?3, build_cursor_group_id = ?4, built_count = ?5
         WHERE source_scope = ?1 AND stage = 'build'
           AND completed_page_ordinal = ?2 AND built_count = ?6
           AND build_cursor_group_id IS ?7",
        params![
            source_scope,
            progress.completed_page_ordinal,
            next_page,
            last_cursor,
            next_built,
            progress.built_count,
            progress.build_cursor_group_id,
        ],
    )?;
    require_single_progress_update(source_scope, changed)?;
    Ok(pending(CodeReferenceSearchRebuildStage::Build, next_page))
}

fn require_build_complete(source_scope: &str, progress: &Progress) -> Result<(), StorageError> {
    if progress.built_count != progress.build_total_count {
        return Err(progress_count_error(source_scope, "build"));
    }
    Ok(())
}

fn cleanup_page_plan(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<PagePlan<String>, StorageError> {
    streaming_page_plan(
        transaction,
        source_scope,
        progress,
        sql::CLEANUP_SCAN_FIRST,
        sql::CLEANUP_SCAN_AFTER,
        sql::CLEANUP_FETCH_CURSOR,
        progress.cleanup_cursor_record_id.as_deref(),
    )
}

fn discovery_page_plan(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<PagePlan<String>, StorageError> {
    streaming_page_plan(
        transaction,
        source_scope,
        progress,
        sql::DISCOVERY_SCAN_FIRST,
        sql::DISCOVERY_SCAN_AFTER,
        sql::DISCOVERY_FETCH_CURSOR,
        progress.discovery_cursor_reference_id.as_deref(),
    )
}

fn build_page_plan(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
) -> Result<PagePlan<String>, StorageError> {
    streaming_page_plan(
        transaction,
        source_scope,
        progress,
        sql::BUILD_SCAN_FIRST,
        sql::BUILD_SCAN_AFTER,
        sql::BUILD_FETCH_CURSOR,
        progress.build_cursor_group_id.as_deref(),
    )
}

fn streaming_page_plan(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
    first_sql: &str,
    after_sql: &str,
    fetch_cursor_sql: &str,
    previous_cursor: Option<&str>,
) -> Result<PagePlan<String>, StorageError> {
    let control_bytes = grouped_checkpoint_bytes(transaction, source_scope)?
        .checked_add(progress_row_bytes_without_active_cursor(
            source_scope,
            progress,
        ))
        .ok_or_else(|| progress_byte_overflow(source_scope))?;
    require_quantum_bytes(
        source_scope,
        "reference-search",
        progress.page_byte_limit,
        control_bytes,
    )?;
    let mut statement = transaction.prepare(if previous_cursor.is_some() {
        after_sql
    } else {
        first_sql
    })?;
    let mut rows = match previous_cursor {
        Some(cursor) => {
            statement.query(params![source_scope, cursor, progress.page_document_limit])?
        }
        None => statement.query(params![source_scope, progress.page_document_limit])?,
    };
    let mut row_count = 0usize;
    let mut owner_bytes = 0usize;
    let mut first_row_bytes = None;
    let mut last_candidate_key = None;
    while let Some(row) = rows.next()? {
        let candidate_key = row.get::<_, i64>(0)?;
        let cursor_bytes = row.get::<_, usize>(1)?;
        let row_bytes = row.get::<_, usize>(2)?;
        let quantum_bytes = control_bytes
            .checked_add(owner_bytes)
            .and_then(|bytes| bytes.checked_add(row_bytes))
            .and_then(|bytes| bytes.checked_add(cursor_bytes))
            .ok_or_else(|| progress_byte_overflow(source_scope))?;
        first_row_bytes.get_or_insert(quantum_bytes);
        if quantum_bytes > progress.page_byte_limit {
            break;
        }
        owner_bytes = owner_bytes
            .checked_add(row_bytes)
            .ok_or_else(|| progress_byte_overflow(source_scope))?;
        row_count = checked_add(row_count, 1, "streamed page row count")?;
        last_candidate_key = Some(candidate_key);
    }
    let last_cursor = last_candidate_key
        .map(|candidate_key| {
            transaction.query_row(
                fetch_cursor_sql,
                params![source_scope, candidate_key],
                |row| row.get::<_, String>(0),
            )
        })
        .transpose()?;
    Ok(PagePlan {
        row_count,
        last_cursor,
        first_row_bytes,
    })
}

fn insert_group_search_page(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress: &Progress,
    planned_last_cursor: &str,
) -> Result<usize, StorageError> {
    require_consecutive_search_rowids(transaction)?;
    let inserted = match progress.build_cursor_group_id.as_deref() {
        Some(cursor) => transaction.execute(
            sql::BUILD_INSERT_SEARCH_AFTER,
            params![source_scope, cursor, planned_last_cursor],
        )?,
        None => transaction.execute(
            sql::BUILD_INSERT_SEARCH_FIRST,
            params![source_scope, planned_last_cursor],
        )?,
    };
    if inserted == 0 {
        return Ok(0);
    }
    let inserted_i64 = i64::try_from(inserted).map_err(|_| {
        StorageError::Invariant(format!(
            "reference-search build page for scope '{source_scope}' exceeds SQLite row-count capacity"
        ))
    })?;
    let last_search_rowid = transaction.last_insert_rowid();
    let first_exclusive_search_rowid = last_search_rowid
        .checked_sub(inserted_i64)
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "reference-search build rowid interval underflows before {last_search_rowid} for {inserted} rows in scope '{source_scope}'"
            ))
        })?;
    let interval_count = transaction.query_row(
        sql::BUILD_INTERVAL_COUNT,
        params![first_exclusive_search_rowid, last_search_rowid],
        |row| row.get::<_, i64>(0),
    )?;
    if interval_count != inserted_i64 {
        return Err(StorageError::Invariant(format!(
            "reference-search build rowid interval ({first_exclusive_search_rowid}, {last_search_rowid}] contains {interval_count} rows for {inserted} groups in scope '{source_scope}'"
        )));
    }
    let inserted_metadata = transaction.execute(
        sql::BUILD_INSERT_METADATA,
        params![
            first_exclusive_search_rowid,
            last_search_rowid,
            source_scope,
        ],
    )?;
    if inserted_metadata != inserted {
        return Err(StorageError::Invariant(format!(
            "reference-search build persisted {inserted} FTS rows but {inserted_metadata} metadata owners for scope '{source_scope}'"
        )));
    }
    Ok(inserted)
}

fn load_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Progress, StorageError> {
    transaction
        .query_row(
            "SELECT projection_version, stage, completed_page_ordinal, cleanup_cursor_rowid,
                    cleanup_cursor_record_id, discovery_cursor_reference_id, build_cursor_group_id,
                    expected_reference_count, cleanup_total_count,
                    discovered_reference_count, discovered_group_count, build_total_count,
                    cleaned_count, built_count, page_document_limit, page_byte_limit
             FROM code_repository_reference_search_progress WHERE source_scope = ?1",
            params![source_scope],
            |row| {
                let stage = match row.get::<_, String>(1)?.as_str() {
                    "cleanup" => CodeReferenceSearchRebuildStage::Cleanup,
                    "discover" => CodeReferenceSearchRebuildStage::Discover,
                    "build" => CodeReferenceSearchRebuildStage::Build,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(Progress {
                    projection_version: row.get(0)?,
                    stage,
                    completed_page_ordinal: row.get(2)?,
                    cleanup_cursor_rowid: row.get(3)?,
                    cleanup_cursor_record_id: row.get(4)?,
                    discovery_cursor_reference_id: row.get(5)?,
                    build_cursor_group_id: row.get(6)?,
                    expected_reference_count: row.get(7)?,
                    cleanup_total_count: row.get(8)?,
                    discovered_reference_count: row.get(9)?,
                    discovered_group_count: row.get(10)?,
                    build_total_count: row.get(11)?,
                    cleaned_count: row.get(12)?,
                    built_count: row.get(13)?,
                    page_document_limit: row.get(14)?,
                    page_byte_limit: row.get(15)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "reference-search progress for scope '{source_scope}' is missing"
            ))
        })
}

fn require_progress_matches_checkpoint(
    source_scope: &str,
    progress: &Progress,
    checkpoint: CodeReferenceSearchRebuild,
) -> Result<(), StorageError> {
    let (stage_total, processed, cursor_shape_is_valid) = match progress.stage {
        CodeReferenceSearchRebuildStage::Cleanup => (
            progress.cleanup_total_count,
            progress.cleaned_count,
            progress.discovered_reference_count == 0
                && progress.built_count == 0
                && progress.discovery_cursor_reference_id.is_none()
                && progress.build_cursor_group_id.is_none()
                && progress.cleanup_cursor_rowid.is_none()
                && progress.cleaned_count == progress.cleanup_total_count
                && ((progress.cleaned_count == 0) == progress.cleanup_cursor_record_id.is_none()),
        ),
        CodeReferenceSearchRebuildStage::Discover => (
            progress.expected_reference_count,
            progress.discovered_reference_count,
            progress.cleaned_count == progress.cleanup_total_count
                && progress.cleanup_cursor_rowid.is_none()
                && progress.cleanup_cursor_record_id.is_none()
                && progress.build_total_count == 0
                && progress.built_count == 0
                && progress.build_cursor_group_id.is_none()
                && ((progress.discovered_reference_count == 0)
                    == progress.discovery_cursor_reference_id.is_none()),
        ),
        CodeReferenceSearchRebuildStage::Build => (
            progress.build_total_count,
            progress.built_count,
            progress.cleaned_count == progress.cleanup_total_count
                && progress.cleanup_cursor_rowid.is_none()
                && progress.cleanup_cursor_record_id.is_none()
                && progress.discovered_reference_count == progress.expected_reference_count
                && progress.discovery_cursor_reference_id.is_none()
                && ((progress.built_count == 0) == progress.build_cursor_group_id.is_none()),
        ),
    };
    if checkpoint.protocol_version != REFERENCE_SEARCH_PROJECTION_VERSION as u32
        || checkpoint.stage != progress.stage
        || checkpoint.completed_page_ordinal != progress.completed_page_ordinal
        || progress.projection_version != REFERENCE_SEARCH_PROJECTION_VERSION
        || progress.completed_page_ordinal > stage_total
        || progress.completed_page_ordinal > processed
        || progress.cleaned_count > progress.cleanup_total_count
        || progress.discovered_reference_count > progress.expected_reference_count
        || progress.built_count > progress.build_total_count
        || progress.page_document_limit == 0
        || progress.page_document_limit > PAGE_DOCUMENT_HARD_LIMIT
        || progress.page_byte_limit == 0
        || progress.page_byte_limit > PAGE_BYTE_HARD_LIMIT
        || !cursor_shape_is_valid
    {
        return Err(StorageError::Invariant(format!(
            "reference-search progress for scope '{source_scope}' does not match its canonical checkpoint"
        )));
    }
    Ok(())
}

fn durable_resource_budget(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<CodeIndexResourceBudget, StorageError> {
    let encoded = transaction
        .query_row(
            "SELECT resource_budget_json FROM code_repository_index_checkpoints
             WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "reference-search progress for scope '{source_scope}' has no durable resource budget"
            ))
        })?;
    let budget = serde_json::from_str::<CodeIndexResourceBudget>(&encoded).map_err(|error| {
        StorageError::Invariant(format!(
            "reference-search progress for scope '{source_scope}' has an invalid durable resource budget: {error}"
        ))
    })?;
    CodeIndexResourceBudget::new(
        budget.max_files_per_batch,
        budget.max_bytes_per_batch,
        budget.max_rows_per_batch,
    )
    .map_err(|error| {
        StorageError::Invariant(format!(
            "reference-search progress for scope '{source_scope}' has an invalid durable resource budget: {error}"
        ))
    })
}

fn grouped_checkpoint_bytes(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<usize, StorageError> {
    let maximum_state =
        code_reference_search_rebuild_state(CodeReferenceSearchRebuildStage::Discover, usize::MAX);
    checkpoint_row_bytes(transaction, source_scope, &maximum_state)
}

fn initial_progress_row_bytes(source_scope: &str) -> usize {
    source_scope.len() + "discover".len() + PROGRESS_RECORD_NON_TEXT_BYTES
}

fn progress_row_bytes_without_active_cursor(source_scope: &str, progress: &Progress) -> usize {
    let all_cursor_bytes = progress
        .cleanup_cursor_record_id
        .as_deref()
        .map_or(0, str::len)
        .saturating_add(
            progress
                .discovery_cursor_reference_id
                .as_deref()
                .map_or(0, str::len),
        )
        .saturating_add(
            progress
                .build_cursor_group_id
                .as_deref()
                .map_or(0, str::len),
        );
    let active_cursor_bytes = match progress.stage {
        CodeReferenceSearchRebuildStage::Cleanup => progress
            .cleanup_cursor_record_id
            .as_deref()
            .map_or(0, str::len),
        CodeReferenceSearchRebuildStage::Discover => progress
            .discovery_cursor_reference_id
            .as_deref()
            .map_or(0, str::len),
        CodeReferenceSearchRebuildStage::Build => progress
            .build_cursor_group_id
            .as_deref()
            .map_or(0, str::len),
    };
    initial_progress_row_bytes(source_scope)
        .saturating_add(all_cursor_bytes.saturating_sub(active_cursor_bytes))
}

fn manifest_row_bytes(source_scope: &str) -> Result<usize, StorageError> {
    source_scope
        .len()
        .checked_add(MANIFEST_RECORD_NON_TEXT_BYTES)
        .ok_or_else(|| progress_byte_overflow(source_scope))
}

fn progress_byte_overflow(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "reference-search control bytes for scope '{source_scope}' exceed platform capacity"
    ))
}

fn require_first_row_within_byte_limit<T>(
    source_scope: &str,
    stage: &str,
    progress: &Progress,
    plan: &PagePlan<T>,
) -> Result<(), StorageError> {
    if plan
        .first_row_bytes
        .is_some_and(|bytes| bytes > progress.page_byte_limit)
    {
        return Err(StorageError::CapacityExceeded(format!(
            "reference-search {stage} row for scope '{source_scope}' exceeds the bounded page byte limit {}",
            progress.page_byte_limit
        )));
    }
    Ok(())
}

fn checked_add(left: usize, right: usize, label: &str) -> Result<usize, StorageError> {
    left.checked_add(right)
        .ok_or_else(|| StorageError::Invariant(format!("reference-search {label} overflowed")))
}

fn require_single_progress_update(source_scope: &str, changed: usize) -> Result<(), StorageError> {
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "reference-search progress for scope '{source_scope}' changed during its page transaction"
    )))
}

fn progress_count_error(source_scope: &str, stage: &str) -> StorageError {
    StorageError::Invariant(format!(
        "reference-search {stage} counts for scope '{source_scope}' do not match durable progress"
    ))
}

const fn pending(
    stage: CodeReferenceSearchRebuildStage,
    completed_page_ordinal: usize,
) -> ReferenceSearchAdvance {
    ReferenceSearchAdvance::Pending {
        stage,
        completed_page_ordinal,
    }
}
