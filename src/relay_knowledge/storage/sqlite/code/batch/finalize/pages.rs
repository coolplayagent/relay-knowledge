//! Shared row/byte admission for durable finalization pages.

use rusqlite::{Transaction, params};

use crate::{domain::CodeIndexResourceBudget, storage::StorageError};

pub(super) const FINALIZATION_PAGE_DOCUMENT_HARD_LIMIT: usize = 32_768;
pub(super) const FINALIZATION_PAGE_BYTE_HARD_LIMIT: usize = 16 * 1024 * 1024;
const FINALIZATION_PAGE_CONTROL_MUTATIONS: usize = 2;
// Eight integer payloads plus their serial types, ten worst-case text serial
// types, and the SQLite record-header length varint.
const CHECKPOINT_RECORD_NON_TEXT_BYTES: usize = 8 * 9 + 10 * 9 + 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FinalizationPageLimits {
    pub(super) document_limit: usize,
    pub(super) byte_limit: usize,
}

impl FinalizationPageLimits {
    /// Reserves both durable control mutations (progress plus checkpoint CAS)
    /// after charging every projected owner mutation for a document.
    pub(super) fn derive(
        source_scope: &str,
        owner: &str,
        resource_budget: CodeIndexResourceBudget,
        owner_mutations_per_document: usize,
    ) -> Result<Self, StorageError> {
        if owner_mutations_per_document == 0 {
            return Err(StorageError::Invariant(format!(
                "{owner} finalization pages for scope '{source_scope}' require a positive owner-mutation charge"
            )));
        }
        let document_limit = resource_budget
            .max_rows_per_batch
            .saturating_sub(FINALIZATION_PAGE_CONTROL_MUTATIONS)
            .saturating_div(owner_mutations_per_document)
            .min(FINALIZATION_PAGE_DOCUMENT_HARD_LIMIT);
        let byte_limit = resource_budget
            .max_bytes_per_batch
            .min(FINALIZATION_PAGE_BYTE_HARD_LIMIT);
        if document_limit == 0 || byte_limit == 0 {
            return Err(StorageError::CapacityExceeded(format!(
                "{owner} finalization pages for scope '{source_scope}' exceed the configured row or byte budget"
            )));
        }
        Ok(Self {
            document_limit,
            byte_limit,
        })
    }

    pub(super) fn from_persisted(
        source_scope: &str,
        owner: &str,
        document_limit: usize,
        byte_limit: usize,
    ) -> Result<Self, StorageError> {
        if document_limit == 0
            || document_limit > FINALIZATION_PAGE_DOCUMENT_HARD_LIMIT
            || byte_limit == 0
            || byte_limit > FINALIZATION_PAGE_BYTE_HARD_LIMIT
        {
            return Err(StorageError::Invariant(format!(
                "{owner} finalization progress for scope '{source_scope}' contains invalid row or byte limits"
            )));
        }
        Ok(Self {
            document_limit,
            byte_limit,
        })
    }
}

/// Computes a conservative full-record byte bound for the checkpoint after
/// replacing its state. Text fields are measured in SQLite; numeric and record
/// headers are charged at their maximum encoded width.
pub(super) fn checkpoint_row_bytes(
    transaction: &Transaction<'_>,
    source_scope: &str,
    next_state: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "SELECT length(CAST(source_scope AS BLOB))
                    + length(CAST(repository_id AS BLOB))
                    + length(CAST(?2 AS BLOB))
                    + length(CAST(resolved_commit_sha AS BLOB))
                    + length(CAST(tree_hash AS BLOB))
                    + length(CAST(path_filters_json AS BLOB))
                    + length(CAST(language_filters_json AS BLOB))
                    + length(CAST(coalesce(last_path, '') AS BLOB))
                    + length(CAST(resource_budget_json AS BLOB))
                    + length(CAST(coalesce(error_message, '') AS BLOB))
                    + ?3
             FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            params![source_scope, next_state, CHECKPOINT_RECORD_NON_TEXT_BYTES],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn require_quantum_bytes(
    source_scope: &str,
    owner: &str,
    byte_limit: usize,
    projected_bytes: usize,
) -> Result<(), StorageError> {
    if projected_bytes > byte_limit {
        return Err(StorageError::CapacityExceeded(format!(
            "{owner} finalization quantum for scope '{source_scope}' exceeds its durable byte limit"
        )));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TextPagePlan {
    pub(super) row_count: usize,
    pub(super) mutation_count: usize,
    pub(super) last_cursor: Option<String>,
    pub(super) first_row_bytes: Option<usize>,
}

pub(super) fn require_admitted_page(
    source_scope: &str,
    owner: &str,
    limits: FinalizationPageLimits,
    plan: &TextPagePlan,
) -> Result<(), StorageError> {
    if plan.row_count > limits.document_limit {
        return Err(StorageError::Invariant(format!(
            "{owner} finalization page for scope '{source_scope}' exceeded its durable row limit"
        )));
    }
    if plan.mutation_count > plan.row_count {
        return Err(StorageError::Invariant(format!(
            "{owner} finalization page for scope '{source_scope}' projected more mutations than scanned rows"
        )));
    }
    if plan.last_cursor.is_none()
        && plan
            .first_row_bytes
            .is_some_and(|bytes| bytes > limits.byte_limit)
    {
        return Err(StorageError::CapacityExceeded(format!(
            "{owner} finalization row for scope '{source_scope}' exceeds its durable byte limit"
        )));
    }
    if plan.last_cursor.is_some() != (plan.row_count > 0) {
        return Err(StorageError::Invariant(format!(
            "{owner} finalization page for scope '{source_scope}' returned an inconsistent cursor"
        )));
    }
    Ok(())
}
