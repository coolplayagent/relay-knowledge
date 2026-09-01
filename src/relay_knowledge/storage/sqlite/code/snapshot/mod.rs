use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{CodeIndexProgressSummary, CodeIndexSnapshot, CodeIndexSummary},
    storage::StorageError,
};

use super::{
    SearchDocumentInserter,
    cleanup::{count_code_rows, delete_path_indexes},
    report,
};

mod admission;
mod candidate_paths;
mod durable_clone;
mod durable_delta;
mod durable_handoff;
mod fingerprints;
mod import_compat;
mod reference_projection;
mod repository_import;
mod scope_tables;
mod search_copy;
mod snapshot_import;

pub(super) use candidate_paths::{
    file_candidate_paths_for_query_scope, file_candidate_paths_for_scope,
};
pub(super) use fingerprints::{
    file_fingerprints, file_fingerprints_for_paths, file_fingerprints_for_scope,
};
pub(super) use repository_import::import_repository_from_database;

use self::scope_tables::{CODE_SCOPE_TABLES, CodeScopeTable, REFERENCE_SEARCH_SCOPE_TABLES};

const MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT: usize = 500;

#[cfg(test)]
#[path = "progress_tests.rs"]
mod progress_tests;

pub(super) fn apply_snapshot(
    connection: &mut Connection,
    snapshot: CodeIndexSnapshot,
) -> Result<CodeIndexSummary, StorageError> {
    apply_snapshot_with_fence(connection, snapshot, None)
}

pub(super) fn apply_snapshot_with_fence(
    connection: &mut Connection,
    snapshot: CodeIndexSnapshot,
    fence: Option<&super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexSummary, StorageError> {
    let Some(guard) = fence.filter(|_| !snapshot.full_replace) else {
        return apply_snapshot_attempt(connection, &snapshot, fence);
    };
    if guard.is_worktree_overlay_task(connection)? {
        match apply_snapshot_attempt(connection, &snapshot, fence) {
            Err(StorageError::DurableStagingRequired(_)) => {}
            outcome => return outcome,
        }
    }
    if let Some(advance) = durable_delta::resume(connection, &snapshot, guard)? {
        return delta_advance_result(advance);
    }
    let session = durable_clone::begin_or_resume(connection, &snapshot, guard).map_err(
        |error| match error {
            StorageError::CapacityExceeded(message) => {
                StorageError::DurableStagingRequired(message)
            }
            other => other,
        },
    )?;
    if let Some(completed_steps) = session.pending_owner_step {
        return Err(StorageError::DurableStagingPending {
            completed_steps,
            max_steps: session.max_steps,
        });
    }
    match durable_clone::advance(connection, &session.identity, guard, session.max_steps)? {
        durable_clone::CloneAdvance::Pending { completed_steps } => {
            if completed_steps > session.max_steps {
                return Err(StorageError::Invariant(format!(
                    "incremental clone for scope '{}' exceeded its durable step proof",
                    snapshot.source_scope
                )));
            }
            return Err(StorageError::DurableStagingPending {
                completed_steps,
                max_steps: session.max_steps,
            });
        }
        durable_clone::CloneAdvance::CloneComplete => {}
    }
    delta_advance_result(durable_delta::start(
        connection, &snapshot, &session, guard,
    )?)
}

fn delta_advance_result(
    advance: durable_delta::DeltaAdvance,
) -> Result<CodeIndexSummary, StorageError> {
    match advance {
        durable_delta::DeltaAdvance::Pending {
            completed_steps,
            max_steps,
        } => Err(StorageError::DurableStagingPending {
            completed_steps,
            max_steps,
        }),
        durable_delta::DeltaAdvance::FinalizationRequired => {
            Err(StorageError::DurableFinalizationRequired {
                checkpoint_state: durable_handoff::FINALIZATION_HANDOFF_STATE.to_owned(),
            })
        }
    }
}

fn apply_snapshot_attempt(
    connection: &mut Connection,
    snapshot: &CodeIndexSnapshot,
    fence: Option<&super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexSummary, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&snapshot.repository_id)?;
    }
    let transaction = connection.transaction()?;
    super::tasks::retention_gc::reject_retiring_scope(&transaction, &snapshot.source_scope)?;
    if let Some(fence) = fence {
        fence.validate(&transaction)?;
        super::publication::reject_fenced_active_scope_rebuild(
            &transaction,
            &snapshot.repository_id,
            &snapshot.source_scope,
        )?;
    } else {
        super::tasks::enforce_unfenced_target(
            &transaction,
            &snapshot.repository_id,
            &snapshot.source_scope,
        )?;
    }
    let direct_budget = fence
        .map(|guard| guard.resource_budget(&transaction))
        .transpose()?
        .unwrap_or_default();
    let incremental_plan = if snapshot.full_replace {
        admission::require_fresh_full_snapshot_within_budget(
            &transaction,
            snapshot,
            direct_budget,
        )?;
        None
    } else {
        Some(admission::require_incremental_snapshot_within_budget(
            &transaction,
            snapshot,
            direct_budget,
        )?)
    };
    super::schema::prepare_query_indexes_for_empty_owners(&transaction)?;
    super::schema::require_code_query_indexes_for_fact_publication(&transaction)?;
    if let Some(plan) = incremental_plan.as_ref() {
        if plan.clone_base {
            for table in CODE_SCOPE_TABLES
                .iter()
                .chain(REFERENCE_SEARCH_SCOPE_TABLES)
            {
                clone_code_table(
                    &transaction,
                    table,
                    &plan.base_scope,
                    &snapshot.source_scope,
                )?;
            }
            search_copy::clone_exact_search_documents(
                &transaction,
                &plan.base_scope,
                &snapshot.source_scope,
                plan.owned_search_document_count,
            )?;
        }
        delete_path_indexes(
            &transaction,
            &snapshot.source_scope,
            snapshot
                .deleted_paths
                .iter()
                .map(String::as_str)
                .chain(snapshot.files.iter().map(|file| file.path.as_str())),
        )?;
    } else {
        reference_projection::require_full_grouped_projection_within_budget(
            &transaction,
            snapshot,
            direct_budget,
        )?;
    }

    for file in &snapshot.files {
        transaction.execute(
            "
            INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
                line_count, parse_status, is_generated, degraded_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                file.is_generated,
                file.degraded_reason,
            ],
        )?;
    }
    let file_languages_by_path = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    super::symbols::insert_records(&transaction, &snapshot.symbols)?;
    let mut search_inserter = SearchDocumentInserter::new(&transaction)?;
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
    }
    reference_projection::insert_direct_reference_search_projection(
        &transaction,
        snapshot,
        &file_languages_by_path,
        direct_budget,
    )?;
    insert_imports_calls_chunks_diagnostics(&transaction, snapshot, &mut search_inserter)?;
    search_inserter.finish()?;
    stage_repository_after_snapshot(&transaction, snapshot, fence.is_some())?;
    repository_import::require_grouped_reference_projection(&transaction, &snapshot.source_scope)?;
    // Resolve workspace-level cross-repo imports when monorepo workspaces
    // were detected during snapshot build.  No-op when workspaces is empty.
    super::workspace::resolve_workspace_imports(
        &transaction,
        &snapshot.workspaces,
        &snapshot.repository_id,
        &snapshot.source_scope,
    )?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &snapshot.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    let status =
        super::status::repository_scope_status_by_source_scope(connection, &snapshot.source_scope)?
            .ok_or_else(|| {
                StorageError::InvalidInput(
                    "code repository scope is missing after index".to_owned(),
                )
            })?;
    let symbol_generation_counts =
        report::scope_symbol_generation_counts(connection, &snapshot.source_scope)?;

    Ok(CodeIndexSummary {
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        base_resolved_commit_sha: snapshot.base_resolved_commit_sha.clone(),
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        indexed_file_count: status.indexed_file_count,
        changed_path_count: snapshot.changed_path_count,
        skipped_unchanged_count: snapshot.skipped_unchanged_count,
        deleted_path_count: snapshot.deleted_paths.len(),
        symbol_count: status.symbol_count,
        handwritten_symbol_count: symbol_generation_counts.handwritten,
        generated_symbol_count: symbol_generation_counts.generated,
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
                .saturating_add(snapshot.dependencies.len())
                .saturating_add(snapshot.calls.len())
                .saturating_add(snapshot.feature_flags.len())
                .saturating_add(snapshot.routes.len())
                .saturating_add(snapshot.chunks.len())
                .saturating_add(snapshot.diagnostics.len()),
            skipped_file_count: snapshot.skipped_unchanged_count,
            degraded_file_count: snapshot.diagnostics.len(),
            batch_count: 1,
            checkpoint_file_count: snapshot.files.len(),
            resource_budget: direct_budget,
        },
    })
}

fn clone_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &CodeScopeTable,
    base_scope: &str,
    target_scope: &str,
) -> Result<(), StorageError> {
    let selected_columns = table.columns.replacen("source_scope", "?2", 1);
    transaction.execute(
        &format!(
            "INSERT INTO {table_name} ({columns})
             SELECT {selected_columns} FROM {table_name} WHERE source_scope = ?1",
            table_name = table.table,
            columns = table.columns,
        ),
        params![base_scope, target_scope],
    )?;

    Ok(())
}

fn insert_imports_calls_chunks_diagnostics<'t>(
    transaction: &rusqlite::Transaction<'t>,
    snapshot: &CodeIndexSnapshot,
    search_inserter: &mut SearchDocumentInserter<'t>,
) -> Result<(), StorageError> {
    let file_languages_by_path = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let symbol_signatures_by_snapshot_id =
        call_symbol_signatures_by_snapshot_id(transaction, snapshot)?;
    let mut import_statement = transaction.prepare(
        "
        INSERT INTO code_repository_imports (
            repository_id, source_scope, import_id, file_id, path, module, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;
    for import in &snapshot.imports {
        import_statement.execute(params![
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
        ])?;
        search_inserter.insert(
            &import.source_scope,
            "import",
            &import.import_id,
            &import.path,
            file_languages_by_path
                .get(import.path.as_str())
                .copied()
                .unwrap_or_default(),
            [
                import.module.as_str(),
                import.target_hint.as_deref().unwrap_or_default(),
                import.path.as_str(),
            ],
        )?;
    }
    let mut call_statement = transaction.prepare(
        "
        INSERT INTO code_repository_calls (
            repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id,
            caller_name, callee_symbol_snapshot_id, callee_name, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
    )?;
    for call in &snapshot.calls {
        let caller_symbol =
            call.caller_symbol_snapshot_id
                .as_deref()
                .and_then(|symbol_snapshot_id| {
                    symbol_signatures_by_snapshot_id.get(symbol_snapshot_id)
                });
        let callee_symbol =
            call.callee_symbol_snapshot_id
                .as_deref()
                .and_then(|symbol_snapshot_id| {
                    symbol_signatures_by_snapshot_id.get(symbol_snapshot_id)
                });
        call_statement.execute(params![
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
        ])?;
        search_inserter.insert(
            &call.source_scope,
            "call",
            &call.call_id,
            &call.path,
            file_languages_by_path
                .get(call.path.as_str())
                .copied()
                .unwrap_or_default(),
            [
                call.caller_name.as_deref().unwrap_or_default(),
                call.callee_name.as_str(),
                call.target_hint.as_deref().unwrap_or_default(),
                caller_symbol.map_or("", String::as_str),
                callee_symbol.map_or("", String::as_str),
                call.path.as_str(),
            ],
        )?;
    }
    super::batch::dependencies::insert_dependency_records(transaction, &snapshot.dependencies)?;
    super::routes::insert_records(transaction, &snapshot.routes)?;
    let mut chunk_statement = transaction.prepare(
        "
        INSERT INTO code_repository_chunks (
            repository_id, source_scope, chunk_id, file_id, path, language_id, content,
            byte_start, byte_end, line_start, line_end, symbol_snapshot_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;
    for chunk in &snapshot.chunks {
        chunk_statement.execute(params![
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
        ])?;
        search_inserter.insert(
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
    super::feature_flags::insert_records(transaction, &snapshot.feature_flags)?;
    let mut diagnostic_statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_file_diagnostics
            (repository_id, source_scope, path, parse_status, message)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
    )?;
    for diagnostic in &snapshot.diagnostics {
        diagnostic_statement.execute(params![
            diagnostic.repository_id,
            diagnostic.source_scope,
            diagnostic.path,
            diagnostic.parse_status.as_str(),
            diagnostic.message,
        ])?;
    }
    let mut tombstone_statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_path_tombstones
            (repository_id, source_scope, old_path, new_path, base_ref, head_ref)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
    )?;
    for tombstone in &snapshot.tombstones {
        tombstone_statement.execute(params![
            tombstone.repository_id,
            tombstone.source_scope,
            tombstone.old_path,
            tombstone.new_path,
            tombstone.base_ref,
            tombstone.head_ref,
        ])?;
    }

    Ok(())
}

fn call_symbol_signatures_by_snapshot_id(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<BTreeMap<String, String>, StorageError> {
    let mut requested_symbol_ids = BTreeSet::new();
    for call in &snapshot.calls {
        if let Some(symbol_snapshot_id) = call.caller_symbol_snapshot_id.as_deref() {
            requested_symbol_ids.insert(symbol_snapshot_id);
        }
        if let Some(symbol_snapshot_id) = call.callee_symbol_snapshot_id.as_deref() {
            requested_symbol_ids.insert(symbol_snapshot_id);
        }
    }
    if requested_symbol_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut signatures = snapshot
        .symbols
        .iter()
        .filter(|symbol| requested_symbol_ids.contains(symbol.symbol_snapshot_id.as_str()))
        .map(|symbol| (symbol.symbol_snapshot_id.clone(), symbol.signature.clone()))
        .collect::<BTreeMap<_, _>>();
    let missing_symbol_ids = requested_symbol_ids
        .into_iter()
        .filter(|symbol_snapshot_id| !signatures.contains_key(*symbol_snapshot_id))
        .collect::<Vec<_>>();
    if missing_symbol_ids.is_empty() {
        return Ok(signatures);
    }

    for symbol_id_chunk in missing_symbol_ids.chunks(MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT)
    {
        let placeholders = std::iter::repeat_n("?", symbol_id_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(symbol_id_chunk.len() + 1);
        values.push(Value::Text(snapshot.source_scope.clone()));
        values.extend(
            symbol_id_chunk
                .iter()
                .map(|symbol_snapshot_id| Value::Text((*symbol_snapshot_id).to_owned())),
        );
        let mut statement = transaction.prepare(&format!(
            "
            SELECT symbol_snapshot_id, signature
            FROM code_repository_symbols
            WHERE source_scope = ? AND symbol_snapshot_id IN ({placeholders})
            "
        ))?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (symbol_snapshot_id, signature) = row?;
            signatures.insert(symbol_snapshot_id, signature);
        }
    }

    Ok(signatures)
}

fn stage_repository_after_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    defer_until_software_projection: bool,
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
    crate::storage::sqlite::code::publication::stage(
        transaction,
        &crate::storage::sqlite::code::publication::ScopePublication {
            repository_id: &snapshot.repository_id,
            source_scope: &snapshot.source_scope,
            resolved_commit_sha: &snapshot.resolved_commit_sha,
            tree_hash: &snapshot.tree_hash,
            path_filters_json: &path_filters_json,
            language_filters_json: &language_filters_json,
            indexed_file_count: file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason: degraded_reason.as_deref(),
        },
        defer_until_software_projection,
    )?;

    Ok(())
}
