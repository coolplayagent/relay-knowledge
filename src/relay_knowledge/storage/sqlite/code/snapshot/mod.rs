use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeIndexProgressSummary, CodeIndexSnapshot, CodeIndexSummary,
        code_snapshot_expected_scope_id, code_snapshot_scope_is_fact_versioned,
    },
    storage::StorageError,
};

use super::{
    MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT,
    cleanup::{count_code_rows, delete_path_index, delete_path_indexes, delete_scope_index},
    lifecycle::commit_scope,
    report,
    search::{SearchDocumentInserter, backfill_search_metadata_for_scope},
    status::{canonical_filter_values, canonical_path_filters, parse_json_list},
};

mod candidate_paths;
mod fingerprints;
mod import_compat;
mod repository_import;
mod scope_tables;
mod snapshot_import;

use self::scope_tables::{CODE_SCOPE_TABLES, CodeScopeTable};
pub(super) use candidate_paths::{
    file_candidate_paths_for_query_scope, file_candidate_paths_for_scope,
};
pub(super) use fingerprints::{
    file_fingerprints, file_fingerprints_for_paths, file_fingerprints_for_scope,
};
pub(super) use repository_import::import_repository_from_database;

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
    if let Some(fence) = fence {
        fence.validate_repository(&snapshot.repository_id)?;
    }
    let transaction = connection.transaction()?;
    super::tasks::retention_gc::reject_retiring_scope(&transaction, &snapshot.source_scope)?;
    if fence.is_none() {
        super::tasks::enforce_unfenced_target(
            &transaction,
            &snapshot.repository_id,
            &snapshot.source_scope,
        )?;
    }
    if snapshot.full_replace {
        delete_scope_index(&transaction, &snapshot.source_scope)?;
    } else {
        let mut excluded_paths = snapshot.deleted_paths.clone();
        for file in &snapshot.files {
            if !excluded_paths.contains(&file.path) {
                excluded_paths.push(file.path.clone());
            }
        }
        excluded_paths.sort_unstable();
        excluded_paths.dedup();
        let cloned_with_exclusion = clone_active_scope_for_incremental(
            &transaction,
            &snapshot.repository_id,
            &snapshot.source_scope,
            &snapshot.path_filters,
            &snapshot.language_filters,
            snapshot.base_resolved_commit_sha.as_deref(),
            &excluded_paths,
        )?;
        // When the clone excluded changed paths, those old rows are already
        // absent from the new scope and the delete steps would be no-ops.
        // When the clone was skipped (same scope) or ran without exclusion,
        // the old rows are still present and must be deleted before reinsert.
        if !cloned_with_exclusion {
            for path in &snapshot.deleted_paths {
                delete_path_index(&transaction, &snapshot.source_scope, path)?;
            }
            delete_path_indexes(
                &transaction,
                &snapshot.source_scope,
                snapshot.files.iter().map(|file| file.path.as_str()),
            )?;
        }
    }

    let mut file_statement = transaction.prepare(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
            line_count, parse_status, is_generated, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
    )?;
    for file in &snapshot.files {
        file_statement.execute(params![
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
        ])?;
    }
    let file_languages_by_path = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    super::symbols::insert_records(&transaction, &snapshot.symbols)?;
    let mut search_inserter = SearchDocumentInserter::new(&transaction)?;
    let mut reference_statement = transaction.prepare(
        "
        INSERT INTO code_repository_references (
            repository_id, source_scope, reference_id, file_id, path, name, kind,
            target_symbol_snapshot_id, target_hint, resolution_state,
            confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    for reference in &snapshot.references {
        reference_statement.execute(params![
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
        ])?;
        search_inserter.insert(
            &reference.source_scope,
            "reference",
            &reference.reference_id,
            &reference.path,
            file_languages_by_path
                .get(reference.path.as_str())
                .copied()
                .unwrap_or_default(),
            [
                reference.name.as_str(),
                reference.kind.as_str(),
                reference.target_hint.as_deref().unwrap_or_default(),
                reference.path.as_str(),
            ],
        )?;
    }
    drop(file_statement);
    drop(reference_statement);
    insert_imports_calls_chunks_diagnostics(&transaction, &snapshot, &mut search_inserter)?;
    search_inserter.finish()?;
    super::schema::ensure_code_query_indexes(&transaction)?;
    update_repository_after_snapshot(&transaction, &snapshot)?;
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

    let status = super::status::repository_status(connection, &snapshot.repository_id)?
        .ok_or_else(|| {
            StorageError::InvalidInput("code repository status is missing after index".to_owned())
        })?;
    let symbol_generation_counts =
        report::scope_symbol_generation_counts(connection, &snapshot.source_scope)?;

    Ok(CodeIndexSummary {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        base_resolved_commit_sha: snapshot.base_resolved_commit_sha,
        resolved_commit_sha: snapshot.resolved_commit_sha,
        tree_hash: snapshot.tree_hash,
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
            resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        },
    })
}

pub(in crate::storage::sqlite::code) fn clone_active_scope_for_incremental(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    path_filters: &[String],
    language_filters: &[String],
    base_resolved_commit_sha: Option<&str>,
    excluded_paths: &[String],
) -> Result<bool, StorageError> {
    let path_filters_json = serde_json::to_string(path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let requested_path_filters = canonical_path_filters(path_filters);
    let requested_language_filters = canonical_filter_values(language_filters);
    let mut statement = transaction.prepare(
        "
        SELECT source_scope, tree_hash, path_filters_json, language_filters_json
        FROM code_repository_scopes
        WHERE repository_id = ?1
          AND (
              resolved_commit_sha = ?4
              OR EXISTS (
                  SELECT 1
                  FROM code_repository_commit_scopes commit_scope
                  WHERE commit_scope.repository_id = code_repository_scopes.repository_id
                    AND commit_scope.resolved_commit_sha = ?4
                    AND commit_scope.source_scope = code_repository_scopes.source_scope
              )
          )
        ORDER BY
          CASE WHEN path_filters_json = ?2 AND language_filters_json = ?3 THEN 0 ELSE 1 END,
          rowid DESC
        ",
    )?;
    let base_commit = base_resolved_commit_sha.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{repository_id}' incremental snapshot is missing its resolved base commit"
        ))
    })?;
    let rows = statement.query_map(
        params![
            repository_id,
            path_filters_json,
            language_filters_json,
            base_commit
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
    let mut previous_scope = None;
    for row in rows {
        let (scope_id, tree_hash, stored_path_filters, stored_language_filters) = row?;
        if canonical_path_filters(&stored_path_filters) == requested_path_filters
            && canonical_filter_values(&stored_language_filters) == requested_language_filters
            && (!code_snapshot_scope_is_fact_versioned(&scope_id)
                || code_snapshot_expected_scope_id(
                    repository_id,
                    &tree_hash,
                    &stored_path_filters,
                    &stored_language_filters,
                )
                .is_some_and(|expected| expected == scope_id))
        {
            previous_scope = Some(scope_id);
            break;
        }
    }
    let previous_scope = previous_scope.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has no matching indexed scope for incremental filters at the current base commit and code fact version"
        ))
    })?;
    if previous_scope == source_scope {
        return Ok(false);
    }
    delete_scope_index(transaction, source_scope)?;
    for table in CODE_SCOPE_TABLES {
        clone_code_table(
            transaction,
            table,
            &previous_scope,
            source_scope,
            excluded_paths,
        )?;
    }
    backfill_search_metadata_for_scope(transaction, source_scope)?;

    Ok(!excluded_paths.is_empty())
}

fn clone_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &CodeScopeTable,
    previous_scope: &str,
    next_scope: &str,
    excluded_paths: &[String],
) -> Result<(), StorageError> {
    let selected_columns = table.columns.replacen("source_scope", "?2", 1);
    if excluded_paths.is_empty() {
        transaction.execute(
            &format!(
                "INSERT INTO {table} ({columns}) SELECT {selected_columns} FROM {table} WHERE source_scope = ?1",
                table = table.table,
                columns = table.columns,
            ),
            params![previous_scope, next_scope],
        )?;
    } else {
        let placeholders = std::iter::repeat_n("?", excluded_paths.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values: Vec<Value> = Vec::with_capacity(2 + excluded_paths.len());
        values.push(Value::Text(previous_scope.to_owned()));
        values.push(Value::Text(next_scope.to_owned()));
        values.extend(excluded_paths.iter().map(|path| Value::Text(path.clone())));
        transaction.execute(
            &format!(
                "INSERT INTO {table} ({columns}) SELECT {selected_columns} FROM {table} WHERE source_scope = ?1 AND path NOT IN ({placeholders})",
                table = table.table,
                columns = table.columns,
            ),
            params_from_iter(values),
        )?;
    }

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
    commit_scope::preserve_existing_scope_commit(
        transaction,
        &snapshot.repository_id,
        &snapshot.source_scope,
    )?;
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
    commit_scope::record(
        transaction,
        &snapshot.repository_id,
        &snapshot.resolved_commit_sha,
        &snapshot.source_scope,
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
