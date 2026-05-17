use rusqlite::{Connection, params};

#[path = "code_query.rs"]
mod code_query;

#[path = "code_impact.rs"]
mod code_impact;

#[path = "code_report.rs"]
mod code_report;

#[path = "code_schema.rs"]
mod code_schema;

#[path = "code_status.rs"]
mod code_status;

#[path = "code_batch.rs"]
mod code_batch;

#[path = "code_cleanup.rs"]
mod code_cleanup;

#[path = "code_tasks.rs"]
mod code_tasks;

#[path = "code_search.rs"]
mod code_search;

#[cfg(test)]
#[path = "code_tests.rs"]
mod code_tests;

#[cfg(test)]
#[path = "code_batch_finalize_tests.rs"]
mod code_batch_finalize_tests;

#[cfg(test)]
#[path = "code_query_accuracy_tests.rs"]
mod code_query_accuracy_tests;

#[cfg(test)]
#[path = "code_query_import_target_tests.rs"]
mod code_query_import_target_tests;

#[cfg(test)]
#[path = "code_query_line_context_tests.rs"]
mod code_query_line_context_tests;

#[cfg(test)]
#[path = "code_metadata_tests.rs"]
mod code_metadata_tests;

#[cfg(test)]
#[path = "code_tasks_tests.rs"]
mod code_tasks_tests;

use crate::{
    domain::{
        CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch, CodeIndexCheckpoint,
        CodeIndexProgressSummary, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
        CodeRepositoryRegistration, CodeRepositoryReport, CodeRepositoryStatus,
        CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest,
    },
    storage::{CodeImpactChanges, CodeRepositoryStore, StorageError, StorageFuture},
};

use super::SqliteGraphStore;
use code_cleanup::{count_code_rows, delete_path_index, delete_scope_index};
pub(super) use code_search::SearchDocumentInserter;
use code_search::insert_search_document;
use code_status::{canonical_filter_values, canonical_path_filters, parse_json_list};

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    code_schema::initialize_code_schema(connection)
}

impl CodeRepositoryStore for SqliteGraphStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        self.run(move |connection| code_status::upsert_repository(connection, registration))
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run(move |connection| code_status::repository_status(connection, &repository))
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run(move |connection| {
            code_status::repository_scope_status(
                connection,
                &repository,
                &resolved_commit_sha,
                &path_filters,
                &language_filters,
            )
        })
    }

    fn queue_code_index_task(
        &self,
        task: crate::storage::CodeIndexTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::queue_task(connection, task))
    }

    fn claim_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| code_tasks::claim_task(connection, request))
    }

    fn complete_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::complete_task(connection, request))
    }

    fn fail_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::fail_task(connection, request))
    }

    fn code_index_task(
        &self,
        task_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| code_tasks::task_by_id(connection, &task_id))
    }

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| code_tasks::active_task(connection, &repository_id))
    }

    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexCheckpoint>> {
        self.run(move |connection| code_tasks::checkpoint(connection, &source_scope))
    }

    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| code_tasks::retention_status(connection, &repository_id))
    }

    fn prune_code_repository_scopes(
        &self,
        request: crate::storage::CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| code_tasks::prune_scopes(connection, request))
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run(move |connection| file_fingerprints(connection, &repository_id))
    }

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run(move |connection| file_fingerprints_for_scope(connection, &source_scope))
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        self.run(move |connection| apply_snapshot(connection, snapshot))
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| code_batch::begin_session(connection, session))
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| code_batch::apply_batch(connection, batch))
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        self.run(move |connection| code_batch::finalize_session(connection, session))
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run(move |connection| code_query::search_code(connection, request))
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run(move |connection| code_impact::analyze_impact(connection, request, changes))
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        self.run(code_report::repository_totals)
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        self.run(move |connection| code_report::repository_report(connection, &repository))
    }
}

fn file_fingerprints(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE repository_id = ?1
          AND source_scope = (
              SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1
          )
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn file_fingerprints_for_scope(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<Vec<CodeFileFingerprint>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, blob_hash
        FROM code_repository_files
        WHERE source_scope = ?1
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(CodeFileFingerprint {
            path: row.get(0)?,
            blob_hash: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn apply_snapshot(
    connection: &mut Connection,
    snapshot: CodeIndexSnapshot,
) -> Result<CodeIndexSummary, StorageError> {
    let transaction = connection.transaction()?;
    if snapshot.full_replace {
        delete_scope_index(&transaction, &snapshot.source_scope)?;
    } else {
        clone_active_scope_for_incremental(&transaction, &snapshot)?;
        for path in &snapshot.deleted_paths {
            delete_path_index(&transaction, &snapshot.source_scope, path)?;
        }
        for file in &snapshot.files {
            delete_path_index(&transaction, &snapshot.source_scope, &file.path)?;
        }
    }

    for file in &snapshot.files {
        transaction.execute(
            "
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
                line_count, parse_status, degraded_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                file.repository_id,
                file.source_scope,
                file.file_id,
                file.path,
                file.language_id,
                file.blob_hash,
                file.byte_len,
                file.line_count,
                file.parse_status.as_str(),
                file.degraded_reason,
            ],
        )?;
    }
    for symbol in &snapshot.symbols {
        transaction.execute(
            "
            INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
                file_id, path, language_id, name,
                qualified_name, kind, signature, doc_comment, byte_start, byte_end,
                line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ",
            params![
                symbol.repository_id,
                symbol.source_scope,
                symbol.symbol_snapshot_id,
                symbol.canonical_symbol_id,
                symbol.file_id,
                symbol.path,
                symbol.language_id,
                symbol.name,
                symbol.qualified_name,
                symbol.kind,
                symbol.signature,
                symbol.doc_comment,
                symbol.byte_range.start,
                symbol.byte_range.end,
                symbol.line_range.start,
                symbol.line_range.end,
            ],
        )?;
        insert_search_document(
            &transaction,
            &symbol.source_scope,
            "symbol",
            &symbol.symbol_snapshot_id,
            &symbol.path,
            &symbol.language_id,
            [
                symbol.name.as_str(),
                symbol.qualified_name.as_str(),
                symbol.kind.as_str(),
                symbol.signature.as_str(),
                symbol.doc_comment.as_deref().unwrap_or_default(),
                symbol.path.as_str(),
            ],
        )?;
    }
    for reference in &snapshot.references {
        transaction.execute(
            "
            INSERT INTO code_repository_references (
                repository_id, source_scope, reference_id, file_id, path, name, kind,
                target_symbol_snapshot_id, target_hint, resolution_state,
                confidence_basis_points, confidence_tier,
                byte_start, byte_end, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ",
            params![
                reference.repository_id,
                reference.source_scope,
                reference.reference_id,
                reference.file_id,
                reference.path,
                reference.name,
                reference.kind,
                reference.target_symbol_snapshot_id,
                reference.target_hint,
                reference.resolution_state,
                reference.confidence_basis_points,
                reference.confidence_tier,
                reference.byte_range.start,
                reference.byte_range.end,
                reference.line_range.start,
                reference.line_range.end,
            ],
        )?;
        insert_search_document(
            &transaction,
            &reference.source_scope,
            "reference",
            &reference.reference_id,
            &reference.path,
            "",
            [
                reference.name.as_str(),
                reference.kind.as_str(),
                reference.target_hint.as_deref().unwrap_or_default(),
                reference.path.as_str(),
            ],
        )?;
    }
    insert_imports_calls_chunks_diagnostics(&transaction, &snapshot)?;
    update_repository_after_snapshot(&transaction, &snapshot)?;
    transaction.commit()?;

    let status =
        code_status::repository_status(connection, &snapshot.repository_id)?.ok_or_else(|| {
            StorageError::InvalidInput("code repository status is missing after index".to_owned())
        })?;

    Ok(CodeIndexSummary {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        resolved_commit_sha: snapshot.resolved_commit_sha,
        tree_hash: snapshot.tree_hash,
        indexed_file_count: status.indexed_file_count,
        changed_path_count: snapshot.changed_path_count,
        skipped_unchanged_count: snapshot.skipped_unchanged_count,
        deleted_path_count: snapshot.deleted_paths.len(),
        symbol_count: status.symbol_count,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count: snapshot.diagnostics.len(),
        progress: CodeIndexProgressSummary {
            git_file_count: if snapshot.full_replace {
                status.indexed_file_count
            } else {
                snapshot.changed_path_count
            },
            blob_read_count: snapshot.files.len(),
            parsed_file_count: snapshot.files.len(),
            sqlite_write_count: snapshot
                .files
                .len()
                .saturating_add(snapshot.symbols.len())
                .saturating_add(snapshot.references.len())
                .saturating_add(snapshot.imports.len())
                .saturating_add(snapshot.calls.len())
                .saturating_add(snapshot.chunks.len())
                .saturating_add(snapshot.diagnostics.len()),
            skipped_file_count: snapshot.skipped_unchanged_count,
            degraded_file_count: snapshot.diagnostics.len(),
            batch_count: 1,
            checkpoint_file_count: snapshot.files.len(),
            resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        },
    })
}

fn clone_active_scope_for_incremental(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    let path_filters_json = serde_json::to_string(&snapshot.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&snapshot.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let requested_path_filters = canonical_path_filters(&snapshot.path_filters);
    let requested_language_filters = canonical_filter_values(&snapshot.language_filters);
    let mut statement = transaction.prepare(
        "
        SELECT source_scope, path_filters_json, language_filters_json
        FROM code_repository_scopes
        WHERE repository_id = ?1
          AND resolved_commit_sha = ?4
        ORDER BY
          CASE WHEN path_filters_json = ?2 AND language_filters_json = ?3 THEN 0 ELSE 1 END,
          rowid DESC
        ",
    )?;
    let base_commit = snapshot
        .base_resolved_commit_sha
        .as_deref()
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository '{}' incremental snapshot is missing its resolved base commit",
                snapshot.repository_id
            ))
        })?;
    let rows = statement.query_map(
        params![
            snapshot.repository_id,
            path_filters_json,
            language_filters_json,
            base_commit
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                parse_json_list(row.get::<_, String>(1)?)?,
                parse_json_list(row.get::<_, String>(2)?)?,
            ))
        },
    )?;
    let mut previous_scope = None;
    for row in rows {
        let (source_scope, stored_path_filters, stored_language_filters) = row?;
        if canonical_path_filters(&stored_path_filters) == requested_path_filters
            && canonical_filter_values(&stored_language_filters) == requested_language_filters
        {
            previous_scope = Some(source_scope);
            break;
        }
    }
    let previous_scope = previous_scope.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' has no matching indexed scope for incremental filters at the current base commit",
            snapshot.repository_id
        ))
    })?;
    if previous_scope == snapshot.source_scope {
        return Ok(());
    }
    delete_scope_index(transaction, &snapshot.source_scope)?;
    clone_code_table(
        transaction,
        "code_repository_files",
        "repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len, line_count, parse_status, degraded_reason",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_symbols",
        "repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, name, qualified_name, kind, signature, doc_comment, byte_start, byte_end, line_start, line_end",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_references",
        "repository_id, source_scope, reference_id, file_id, path, name, kind, target_symbol_snapshot_id, target_hint, resolution_state, confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_imports",
        "repository_id, source_scope, import_id, file_id, path, module, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_calls",
        "repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id, caller_name, callee_symbol_snapshot_id, callee_name, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_chunks",
        "repository_id, source_scope, chunk_id, file_id, path, language_id, content, byte_start, byte_end, line_start, line_end, symbol_snapshot_id",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_file_diagnostics",
        "repository_id, source_scope, path, parse_status, message",
        &previous_scope,
        &snapshot.source_scope,
    )?;
    clone_code_table(
        transaction,
        "code_repository_search",
        "source_scope, document_kind, record_id, path, language_id, content",
        &previous_scope,
        &snapshot.source_scope,
    )?;

    Ok(())
}

fn clone_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    columns: &'static str,
    previous_scope: &str,
    next_scope: &str,
) -> Result<(), StorageError> {
    let selected_columns = columns.replacen("source_scope", "?2", 1);
    transaction.execute(
        &format!(
            "INSERT INTO {table} ({columns}) SELECT {selected_columns} FROM {table} WHERE source_scope = ?1"
        ),
        params![previous_scope, next_scope],
    )?;

    Ok(())
}

fn insert_imports_calls_chunks_diagnostics(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    for import in &snapshot.imports {
        transaction.execute(
            "
            INSERT INTO code_repository_imports (
                repository_id, source_scope, import_id, file_id, path, module, target_hint,
                resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ",
            params![
                import.repository_id,
                import.source_scope,
                import.import_id,
                import.file_id,
                import.path,
                import.module,
                import.target_hint,
                import.resolution_state,
                import.confidence_basis_points,
                import.confidence_tier,
                import.line_range.start,
                import.line_range.end,
            ],
        )?;
        insert_search_document(
            transaction,
            &import.source_scope,
            "import",
            &import.import_id,
            &import.path,
            "",
            [
                import.module.as_str(),
                import.target_hint.as_deref().unwrap_or_default(),
                import.path.as_str(),
            ],
        )?;
    }
    for call in &snapshot.calls {
        transaction.execute(
            "
            INSERT INTO code_repository_calls (
                repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id,
                caller_name, callee_symbol_snapshot_id, callee_name, target_hint,
                resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ",
            params![
                call.repository_id,
                call.source_scope,
                call.call_id,
                call.file_id,
                call.path,
                call.caller_symbol_snapshot_id,
                call.caller_name,
                call.callee_symbol_snapshot_id,
                call.callee_name,
                call.target_hint,
                call.resolution_state,
                call.confidence_basis_points,
                call.confidence_tier,
                call.line_range.start,
                call.line_range.end,
            ],
        )?;
        insert_search_document(
            transaction,
            &call.source_scope,
            "call",
            &call.call_id,
            &call.path,
            "",
            [
                call.caller_name.as_deref().unwrap_or_default(),
                call.callee_name.as_str(),
                call.target_hint.as_deref().unwrap_or_default(),
                call.path.as_str(),
            ],
        )?;
    }
    for chunk in &snapshot.chunks {
        transaction.execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, language_id, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ",
            params![
                chunk.repository_id,
                chunk.source_scope,
                chunk.chunk_id,
                chunk.file_id,
                chunk.path,
                chunk.language_id,
                chunk.content,
                chunk.byte_range.start,
                chunk.byte_range.end,
                chunk.line_range.start,
                chunk.line_range.end,
                chunk.symbol_snapshot_id,
            ],
        )?;
        insert_search_document(
            transaction,
            &chunk.source_scope,
            "chunk",
            &chunk.chunk_id,
            &chunk.path,
            &chunk.language_id,
            [
                chunk.content.as_str(),
                chunk.symbol_snapshot_id.as_deref().unwrap_or_default(),
                chunk.path.as_str(),
            ],
        )?;
    }
    for diagnostic in &snapshot.diagnostics {
        transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_file_diagnostics
                (repository_id, source_scope, path, parse_status, message)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![
                diagnostic.repository_id,
                diagnostic.source_scope,
                diagnostic.path,
                diagnostic.parse_status.as_str(),
                diagnostic.message,
            ],
        )?;
    }
    for tombstone in &snapshot.tombstones {
        transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_path_tombstones
                (repository_id, source_scope, old_path, new_path, base_ref, head_ref)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                tombstone.repository_id,
                tombstone.source_scope,
                tombstone.old_path,
                tombstone.new_path,
                tombstone.base_ref,
                tombstone.head_ref,
            ],
        )?;
    }

    Ok(())
}

fn update_repository_after_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    let file_count = count_code_rows(transaction, "code_repository_files", &snapshot.source_scope)?;
    let symbol_count = count_code_rows(
        transaction,
        "code_repository_symbols",
        &snapshot.source_scope,
    )?;
    let reference_count = count_code_rows(
        transaction,
        "code_repository_references",
        &snapshot.source_scope,
    )?;
    let chunk_count = count_code_rows(
        transaction,
        "code_repository_chunks",
        &snapshot.source_scope,
    )?;
    let degraded_file_count = count_code_rows(
        transaction,
        "code_repository_file_diagnostics",
        &snapshot.source_scope,
    )?;
    let degraded_reason = (degraded_file_count > 0)
        .then(|| format!("{degraded_file_count} file(s) degraded during code indexing"));
    let path_filters_json = serde_json::to_string(&snapshot.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&snapshot.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    transaction.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            resolved_commit_sha = excluded.resolved_commit_sha,
            tree_hash = excluded.tree_hash,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            indexed_file_count = excluded.indexed_file_count,
            symbol_count = excluded.symbol_count,
            reference_count = excluded.reference_count,
            chunk_count = excluded.chunk_count,
            stale = 0,
            degraded_reason = excluded.degraded_reason
        ",
        params![
            snapshot.source_scope,
            snapshot.repository_id,
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            path_filters_json,
            language_filters_json,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET last_indexed_scope_id = ?2,
            last_indexed_commit = ?3,
            tree_hash = ?4,
            state = 'fresh',
            indexed_file_count = ?5,
            symbol_count = ?6,
            reference_count = ?7,
            chunk_count = ?8,
            stale = 0,
            degraded_reason = ?9
        WHERE repository_id = ?1
        ",
        params![
            snapshot.repository_id,
            snapshot.source_scope,
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;

    Ok(())
}
