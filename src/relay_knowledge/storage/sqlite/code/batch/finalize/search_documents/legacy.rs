//! Upgrades a leased reference-search v1 page to the bounded grouped v2 protocol.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeReferenceSearchRebuild, CodeReferenceSearchRebuildStage,
    },
    storage::StorageError,
};

use super::grouped::{
    REFERENCE_SEARCH_PROJECTION_VERSION, ReferenceSearchAdvance, reference_search_page_limits,
};

pub(super) fn restart_legacy_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
    progress_stage: CodeReferenceSearchRebuildStage,
    completed_page_ordinal: usize,
    expected_reference_count: usize,
    checkpoint: CodeReferenceSearchRebuild,
) -> Result<ReferenceSearchAdvance, StorageError> {
    if checkpoint.protocol_version != 1
        || checkpoint.stage != progress_stage
        || checkpoint.completed_page_ordinal != completed_page_ordinal
    {
        return Err(StorageError::Invariant(format!(
            "legacy reference-search progress for scope '{source_scope}' does not match its checkpoint"
        )));
    }
    let owner_rows_exist = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_reference_search_groups WHERE source_scope = ?1 LIMIT 1
         ) OR EXISTS (
             SELECT 1 FROM code_repository_reference_search_manifests
             WHERE source_scope = ?1 LIMIT 1
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if owner_rows_exist {
        return Err(StorageError::Invariant(format!(
            "legacy reference-search progress for scope '{source_scope}' already owns v2 group rows"
        )));
    }
    let legacy_stage = match progress_stage {
        CodeReferenceSearchRebuildStage::Cleanup => "cleanup",
        CodeReferenceSearchRebuildStage::Build => "build",
        CodeReferenceSearchRebuildStage::Discover => {
            return Err(StorageError::Invariant(format!(
                "legacy reference-search progress for scope '{source_scope}' has a v2-only stage"
            )));
        }
    };
    let (committed_reference_count, resource_budget_json) = transaction
        .query_row(
            "SELECT committed_reference_count, resource_budget_json
             FROM code_repository_index_checkpoints
             WHERE source_scope = ?1",
            params![source_scope],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "legacy reference-search progress for scope '{source_scope}' has no durable checkpoint budget"
            ))
        })?;
    if committed_reference_count != expected_reference_count {
        return Err(StorageError::Invariant(format!(
            "legacy reference-search progress for scope '{source_scope}' does not match its frozen checkpoint reference count"
        )));
    }
    let resource_budget = serde_json::from_str::<CodeIndexResourceBudget>(&resource_budget_json)
        .map_err(|error| {
            StorageError::Invariant(format!(
                "legacy reference-search progress for scope '{source_scope}' has an invalid durable resource budget: {error}"
            ))
        })?;
    let resource_budget = CodeIndexResourceBudget::new(
        resource_budget.max_files_per_batch,
        resource_budget.max_bytes_per_batch,
        resource_budget.max_rows_per_batch,
    )
    .map_err(|error| {
        StorageError::Invariant(format!(
            "legacy reference-search progress for scope '{source_scope}' has an invalid durable resource budget: {error}"
        ))
    })?;
    let (page_document_limit, page_byte_limit) =
        reference_search_page_limits(source_scope, resource_budget)?;
    let changed = transaction.execute(
        "UPDATE code_repository_reference_search_progress
         SET projection_version = ?4, stage = 'cleanup', completed_page_ordinal = 0,
             cleanup_cursor_rowid = NULL, cleanup_cursor_record_id = NULL,
             discovery_cursor_reference_id = NULL,
             build_cursor_group_id = NULL, cleanup_total_count = 0,
             discovered_reference_count = 0, discovered_group_count = 0,
             build_total_count = 0, cleaned_count = 0, built_count = 0,
             page_document_limit = ?5, page_byte_limit = ?6
         WHERE source_scope = ?1 AND projection_version = 1
           AND stage = ?2 AND completed_page_ordinal = ?3",
        params![
            source_scope,
            legacy_stage,
            completed_page_ordinal,
            REFERENCE_SEARCH_PROJECTION_VERSION,
            page_document_limit,
            page_byte_limit,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "legacy reference-search progress for scope '{source_scope}' changed during its restart transaction"
        )));
    }
    Ok(ReferenceSearchAdvance::Pending {
        stage: CodeReferenceSearchRebuildStage::Cleanup,
        completed_page_ordinal: 0,
    })
}
