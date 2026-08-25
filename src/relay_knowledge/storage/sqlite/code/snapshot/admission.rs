//! Bounds direct snapshot publication before the first write.

use std::io::{self, Write};

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeIndexSnapshot, code_snapshot_scope_is_fact_versioned,
        code_snapshot_scope_matches_identity,
    },
    storage::StorageError,
};

use super::{
    super::status::{canonical_filter_values, canonical_path_filters, parse_json_list},
    scope_tables::{CODE_SCOPE_TABLES, CodeScopeTable, REFERENCE_SEARCH_SCOPE_TABLES},
};

const SNAPSHOT_SEARCH_EXPANSION: usize = 8;
pub(super) const ROW_STORAGE_OVERHEAD_BYTES: usize = 128;
const FIXED_PUBLICATION_ROWS: usize = 8;

struct BoundedByteCounter {
    bytes: usize,
    limit: usize,
    identity: super::super::super::evidence_identity::StableIdWriter,
}

pub(super) struct DirectBudgetMeasure {
    source_scope: String,
    budget: CodeIndexResourceBudget,
    rows: usize,
    bytes: usize,
    delta_digest: String,
}

#[derive(Debug)]
pub(super) struct DirectIncrementalPlan {
    pub(super) base_scope: String,
    pub(super) clone_base: bool,
    pub(super) owned_search_document_count: usize,
}

impl DirectBudgetMeasure {
    pub(super) fn delta_digest(&self) -> &str {
        &self.delta_digest
    }
    pub(super) fn remaining_rows(&self) -> usize {
        self.budget.max_rows_per_batch.saturating_sub(self.rows)
    }

    pub(super) fn add_scaled(
        &mut self,
        rows: usize,
        bytes: usize,
        multiplier: usize,
    ) -> Result<(), StorageError> {
        let scaled_rows = rows
            .checked_mul(multiplier)
            .ok_or_else(|| capacity_error(&self.source_scope))?;
        let scaled_bytes = bytes
            .checked_mul(multiplier)
            .ok_or_else(|| capacity_error(&self.source_scope))?;
        self.add(scaled_rows, scaled_bytes)
    }

    fn add(&mut self, rows: usize, bytes: usize) -> Result<(), StorageError> {
        self.rows = self
            .rows
            .checked_add(rows)
            .ok_or_else(|| capacity_error(&self.source_scope))?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| capacity_error(&self.source_scope))?;
        if self.rows > self.budget.max_rows_per_batch
            || self.bytes > self.budget.max_bytes_per_batch
        {
            return Err(capacity_error(&self.source_scope));
        }
        Ok(())
    }
}

impl Write for BoundedByteCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::other("direct snapshot byte counter overflowed"))?;
        if next > self.limit {
            return Err(io::Error::other(
                "direct snapshot serialization exceeded its writer quantum",
            ));
        }
        self.bytes = next;
        self.identity.write_all(buffer)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn require_fresh_full_snapshot_within_budget(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    require_unused_target(transaction, &snapshot.source_scope)?;
    require_no_workspace_projection(transaction, snapshot)?;
    measure_snapshot_insert_surface(snapshot, budget)?;

    Ok(())
}

pub(super) fn require_incremental_snapshot_within_budget(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    budget: CodeIndexResourceBudget,
) -> Result<DirectIncrementalPlan, StorageError> {
    match require_incremental_snapshot_within_budget_inner(transaction, snapshot, budget) {
        Err(StorageError::CapacityExceeded(message)) => {
            Err(StorageError::DurableStagingRequired(message))
        }
        result => result,
    }
}

fn require_incremental_snapshot_within_budget_inner(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    budget: CodeIndexResourceBudget,
) -> Result<DirectIncrementalPlan, StorageError> {
    let affected_path_upper_bound = snapshot
        .files
        .len()
        .checked_add(snapshot.deleted_paths.len())
        .ok_or_else(|| capacity_error(&snapshot.source_scope))?;
    if affected_path_upper_bound > budget.max_files_per_batch {
        return Err(capacity_error(&snapshot.source_scope));
    }
    require_no_workspace_projection(transaction, snapshot)?;
    let base_scope = resolve_incremental_base_scope(transaction, snapshot)?;
    let clone_base = base_scope != snapshot.source_scope;
    if clone_base {
        require_unused_target(transaction, &snapshot.source_scope)?;
    }
    super::repository_import::require_grouped_reference_projection(transaction, &base_scope)?;

    let mut measure = measure_snapshot_insert_surface(snapshot, budget)?;
    let mutation_multiplier = if clone_base { 2 } else { 1 };
    for table in CODE_SCOPE_TABLES
        .iter()
        .chain(REFERENCE_SEARCH_SCOPE_TABLES)
    {
        measure_scope_table(
            transaction,
            table,
            &base_scope,
            &snapshot.source_scope,
            mutation_multiplier,
            &mut measure,
        )?;
    }
    let owned_search_document_count = measure_owned_search_documents(
        transaction,
        &base_scope,
        &snapshot.source_scope,
        mutation_multiplier,
        &mut measure,
    )?;

    Ok(DirectIncrementalPlan {
        base_scope,
        clone_base,
        owned_search_document_count,
    })
}

pub(super) fn measure_snapshot_insert_surface(
    snapshot: &CodeIndexSnapshot,
    budget: CodeIndexResourceBudget,
) -> Result<DirectBudgetMeasure, StorageError> {
    if snapshot.files.len() > budget.max_files_per_batch {
        return Err(capacity_error(&snapshot.source_scope));
    }

    let fact_rows = snapshot
        .files
        .len()
        .saturating_add(snapshot.symbols.len())
        .saturating_add(snapshot.references.len())
        .saturating_add(snapshot.imports.len())
        .saturating_add(snapshot.dependencies.len())
        .saturating_add(snapshot.calls.len())
        .saturating_add(snapshot.feature_flags.len())
        .saturating_add(snapshot.routes.len())
        .saturating_add(snapshot.chunks.len())
        .saturating_add(snapshot.diagnostics.len())
        .saturating_add(snapshot.tombstones.len());
    let reference_search_groups =
        super::reference_projection::reference_search_group_count(snapshot)?;
    let search_documents = snapshot
        .symbols
        .len()
        .saturating_add(snapshot.imports.len())
        .saturating_add(snapshot.dependencies.len())
        .saturating_add(snapshot.calls.len())
        .saturating_add(snapshot.feature_flags.len())
        .saturating_add(snapshot.routes.len())
        .saturating_add(snapshot.chunks.len());
    let insert_rows = fact_rows
        .saturating_add(search_documents.saturating_mul(2))
        .saturating_add(reference_search_groups.saturating_mul(3))
        .saturating_add(FIXED_PUBLICATION_ROWS);
    if insert_rows > budget.max_rows_per_batch {
        return Err(capacity_error(&snapshot.source_scope));
    }

    let mut measure = DirectBudgetMeasure {
        source_scope: snapshot.source_scope.clone(),
        budget,
        rows: 0,
        bytes: 0,
        delta_digest: String::new(),
    };

    let identity_bytes = insert_rows.saturating_mul(
        snapshot
            .source_scope
            .len()
            .saturating_add(ROW_STORAGE_OVERHEAD_BYTES),
    );
    measure.add(insert_rows, identity_bytes)?;
    let remaining_bytes = budget.max_bytes_per_batch.saturating_sub(measure.bytes);
    let mut counter = BoundedByteCounter {
        bytes: 0,
        limit: remaining_bytes / SNAPSHOT_SEARCH_EXPANSION,
        identity: super::super::super::evidence_identity::StableIdWriter::new(),
    };
    serde_json::to_writer(&mut counter, snapshot)
        .map_err(|_| capacity_error(&snapshot.source_scope))?;
    measure.delta_digest = counter.identity.finish("code-incremental-delta");
    measure.add(
        0,
        counter
            .bytes
            .checked_mul(SNAPSHOT_SEARCH_EXPANSION)
            .ok_or_else(|| capacity_error(&snapshot.source_scope))?,
    )?;

    Ok(measure)
}

pub(super) fn require_no_workspace_projection(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    if !snapshot.workspaces.is_empty()
        || super::super::workspace::has_auto_workspace_state(transaction, &snapshot.repository_id)?
    {
        return Err(StorageError::DurableStagingRequired(format!(
            "direct workspace publication for scope '{}' requires the checkpointed full-index pipeline",
            snapshot.source_scope
        )));
    }
    Ok(())
}

pub(super) fn require_unused_target(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let existing_owner = transaction
        .query_row(
            "SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1
             UNION ALL
             SELECT 1 FROM code_repository_index_checkpoints WHERE source_scope = ?1
             LIMIT 1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?;
    if existing_owner.is_some() {
        return Err(StorageError::DurableStagingRequired(format!(
            "direct full replacement of existing scope '{source_scope}' requires the checkpointed full-index pipeline"
        )));
    }
    Ok(())
}

pub(super) fn resolve_incremental_base_scope(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<String, StorageError> {
    let base_commit = snapshot
        .base_resolved_commit_sha
        .as_deref()
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository '{}' incremental snapshot is missing its resolved base commit",
                snapshot.repository_id
            ))
        })?;
    let path_filters_json = serde_json::to_string(&snapshot.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&snapshot.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let requested_path_filters = canonical_path_filters(&snapshot.path_filters);
    let requested_language_filters = canonical_filter_values(&snapshot.language_filters);
    let mut statement = transaction.prepare(
        "SELECT scope.source_scope, scope.tree_hash,
                scope.path_filters_json, scope.language_filters_json
         FROM code_repository_scopes scope
         WHERE scope.repository_id = ?1
           AND scope.stale = 0
           AND scope.retiring = 0
           AND NOT EXISTS (
               SELECT 1 FROM code_repository_scope_gc_jobs job
               WHERE job.repository_id = scope.repository_id
                 AND job.source_scope = scope.source_scope
           )
           AND (
               scope.resolved_commit_sha = ?4
               OR EXISTS (
                   SELECT 1 FROM code_repository_commit_scopes commit_scope
                   WHERE commit_scope.repository_id = scope.repository_id
                     AND commit_scope.resolved_commit_sha = ?4
                     AND commit_scope.source_scope = scope.source_scope
               )
           )
         ORDER BY
           CASE WHEN scope.path_filters_json = ?2
                  AND scope.language_filters_json = ?3 THEN 0 ELSE 1 END,
           scope.source_scope DESC",
    )?;
    let rows = statement.query_map(
        params![
            snapshot.repository_id,
            path_filters_json,
            language_filters_json,
            base_commit,
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                parse_json_list(row.get::<_, String>(2)?)?,
                parse_json_list(row.get::<_, String>(3)?)?,
            ))
        },
    )?;
    for row in rows {
        let (source_scope, tree_hash, stored_path_filters, stored_language_filters) = row?;
        if canonical_path_filters(&stored_path_filters) != requested_path_filters
            || canonical_filter_values(&stored_language_filters) != requested_language_filters
        {
            continue;
        }
        if !code_snapshot_scope_is_fact_versioned(&source_scope)
            || code_snapshot_scope_matches_identity(
                &snapshot.repository_id,
                &tree_hash,
                &stored_path_filters,
                &stored_language_filters,
                &source_scope,
            )
        {
            return Ok(source_scope);
        }
    }

    Err(StorageError::InvalidInput(format!(
        "code repository '{}' has no matching fresh, non-retiring indexed scope for incremental filters at the current base commit and code fact version",
        snapshot.repository_id
    )))
}

fn measure_scope_table(
    transaction: &Transaction<'_>,
    table: &CodeScopeTable,
    base_scope: &str,
    target_scope: &str,
    mutation_multiplier: usize,
    measure: &mut DirectBudgetMeasure,
) -> Result<(), StorageError> {
    require_source_scope_leading_index(transaction, table.table)?;
    let row_limit = bounded_row_limit(measure, mutation_multiplier)?;
    let length_columns = table
        .columns
        .split(',')
        .map(str::trim)
        .map(validated_identifier_length_sql)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let mut statement = transaction.prepare(&format!(
        "SELECT {length_columns} FROM {table_name}
         WHERE source_scope = ?1 LIMIT ?2",
        table_name = table.table,
    ))?;
    let mut rows = statement.query(params![base_scope, row_limit])?;
    while let Some(row) = rows.next()? {
        let mut bytes = target_scope
            .len()
            .checked_add(ROW_STORAGE_OVERHEAD_BYTES)
            .ok_or_else(|| capacity_error(target_scope))?;
        for index in 0..row.as_ref().column_count() {
            let value = row.get::<_, i64>(index)?;
            bytes = bytes
                .checked_add(usize::try_from(value).map_err(|_| capacity_error(target_scope))?)
                .ok_or_else(|| capacity_error(target_scope))?;
        }
        measure.add_scaled(1, bytes, mutation_multiplier)?;
    }
    Ok(())
}

fn measure_owned_search_documents(
    transaction: &Transaction<'_>,
    base_scope: &str,
    target_scope: &str,
    mutation_multiplier: usize,
    measure: &mut DirectBudgetMeasure,
) -> Result<usize, StorageError> {
    require_source_scope_leading_index(transaction, "code_repository_search_metadata")?;
    let row_multiplier = mutation_multiplier
        .checked_mul(2)
        .ok_or_else(|| capacity_error(target_scope))?;
    let row_limit = bounded_row_limit(measure, row_multiplier)?;
    let mut statement = transaction.prepare(
        "SELECT search_row.rowid,
                coalesce(length(CAST(metadata.source_scope AS BLOB)), 0),
                coalesce(length(CAST(metadata.document_kind AS BLOB)), 0),
                coalesce(length(CAST(metadata.record_id AS BLOB)), 0),
                coalesce(length(CAST(metadata.path AS BLOB)), 0),
                coalesce(length(CAST(search_row.language_id AS BLOB)), 0),
                coalesce(length(CAST(search_row.content AS BLOB)), 0)
         FROM code_repository_search_metadata metadata
         LEFT JOIN code_repository_search search_row
           ON search_row.rowid = metadata.search_rowid
          AND search_row.source_scope = metadata.source_scope
          AND search_row.document_kind = metadata.document_kind
          AND search_row.record_id = metadata.record_id
          AND search_row.path = metadata.path
         WHERE metadata.source_scope = ?1
         LIMIT ?2",
    )?;
    let mut rows = statement.query(params![base_scope, row_limit])?;
    let mut document_count = 0usize;
    while let Some(row) = rows.next()? {
        if row.get::<_, Option<i64>>(0)?.is_none() {
            return Err(StorageError::Invariant(format!(
                "code search scope '{base_scope}' has metadata without an exact FTS owner"
            )));
        }
        let mut bytes = target_scope
            .len()
            .checked_add(ROW_STORAGE_OVERHEAD_BYTES.saturating_mul(2))
            .ok_or_else(|| capacity_error(target_scope))?;
        for index in 1..row.as_ref().column_count() {
            let value = row.get::<_, i64>(index)?;
            bytes = bytes
                .checked_add(usize::try_from(value).map_err(|_| capacity_error(target_scope))?)
                .ok_or_else(|| capacity_error(target_scope))?;
        }
        measure.add_scaled(2, bytes, mutation_multiplier)?;
        document_count = document_count
            .checked_add(1)
            .ok_or_else(|| capacity_error(target_scope))?;
    }
    Ok(document_count)
}

fn require_source_scope_leading_index(
    transaction: &Transaction<'_>,
    table: &str,
) -> Result<(), StorageError> {
    let mut indexes = transaction
        .prepare("SELECT name FROM pragma_index_list(?1) WHERE partial = 0 ORDER BY seq")?;
    let names = indexes
        .query_map(params![table], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for name in names {
        let first_column = transaction
            .query_row(
                "SELECT name FROM pragma_index_info(?1) WHERE seqno = 0",
                params![name],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if first_column.as_deref() == Some("source_scope") {
            return Ok(());
        }
    }
    Err(StorageError::Invariant(format!(
        "direct snapshot admission requires table '{table}' to have a non-partial source_scope-leading index"
    )))
}

fn bounded_row_limit(
    measure: &DirectBudgetMeasure,
    mutation_multiplier: usize,
) -> Result<i64, StorageError> {
    if mutation_multiplier == 0 {
        return Err(capacity_error(&measure.source_scope));
    }
    let allowed = measure.remaining_rows() / mutation_multiplier;
    let with_sentinel = allowed
        .checked_add(1)
        .ok_or_else(|| capacity_error(&measure.source_scope))?;
    i64::try_from(with_sentinel).map_err(|_| capacity_error(&measure.source_scope))
}

pub(super) fn validated_identifier_length_sql(column: &str) -> Result<String, StorageError> {
    if column.is_empty()
        || !column
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(StorageError::Invariant(format!(
            "direct snapshot admission received an invalid schema column '{column}'"
        )));
    }
    Ok(format!("coalesce(length(CAST(\"{column}\" AS BLOB)), 0)"))
}

fn capacity_error(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "direct snapshot for scope '{source_scope}' exceeds its writer quantum; use the checkpointed full-index pipeline"
    ))
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
