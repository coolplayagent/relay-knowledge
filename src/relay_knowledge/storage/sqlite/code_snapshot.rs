use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeFileFingerprint, CodeIndexProgressSummary, CodeIndexSnapshot, CodeIndexSummary,
        code_snapshot_expected_scope_id, code_snapshot_scope_is_fact_versioned,
    },
    storage::StorageError,
};

use super::{
    MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT,
    code_cleanup::{count_code_rows, delete_path_index, delete_path_indexes, delete_scope_index},
    code_search::{backfill_search_metadata_for_scope, insert_search_document},
    code_status::{canonical_filter_values, canonical_path_filters, parse_json_list},
};

#[path = "code_snapshot_candidate_paths.rs"]
mod candidate_paths;

#[cfg(test)]
pub(super) use candidate_paths::candidate_path_fts_query;
pub(super) use candidate_paths::{
    file_candidate_paths_for_query_scope, file_candidate_paths_for_scope,
};

const IMPORT_SCHEMA: &str = "relay_import";

struct CodeScopeTable {
    table: &'static str,
    columns: &'static str,
}

const CODE_SCOPE_TABLES: &[CodeScopeTable] = &[
    CodeScopeTable {
        table: "code_repository_files",
        columns: "repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len, line_count, parse_status, degraded_reason",
    },
    CodeScopeTable {
        table: "code_repository_symbols",
        columns: "repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, name, qualified_name, kind, signature, doc_comment, byte_start, byte_end, line_start, line_end",
    },
    CodeScopeTable {
        table: "code_repository_references",
        columns: "repository_id, source_scope, reference_id, file_id, path, name, kind, target_symbol_snapshot_id, target_hint, resolution_state, confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end",
    },
    CodeScopeTable {
        table: "code_repository_imports",
        columns: "repository_id, source_scope, import_id, file_id, path, module, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
    },
    CodeScopeTable {
        table: "code_repository_dependencies",
        columns: "repository_id, source_scope, dependency_id, file_id, path, language_id, ecosystem, package_name, requirement, resolved_version, dependency_group, source_kind, is_lockfile, line_start, line_end, excerpt",
    },
    CodeScopeTable {
        table: "code_repository_calls",
        columns: "repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id, caller_name, callee_symbol_snapshot_id, callee_name, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
    },
    CodeScopeTable {
        table: "code_repository_feature_flags",
        columns: "repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id, name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end, excerpt",
    },
    CodeScopeTable {
        table: "code_repository_chunks",
        columns: "repository_id, source_scope, chunk_id, file_id, path, language_id, content, byte_start, byte_end, line_start, line_end, symbol_snapshot_id",
    },
    CodeScopeTable {
        table: "code_repository_file_diagnostics",
        columns: "repository_id, source_scope, path, parse_status, message",
    },
    CodeScopeTable {
        table: "code_repository_search",
        columns: "source_scope, document_kind, record_id, path, language_id, content",
    },
];

const IMPORTED_DERIVED_SCOPE_TABLES: &[CodeScopeTable] = &[
    CodeScopeTable {
        table: "code_repository_index_checkpoints",
        columns: "source_scope, repository_id, state, resolved_commit_sha, tree_hash, path_filters_json, language_filters_json, total_path_count, parsed_file_count, committed_file_count, committed_symbol_count, committed_reference_count, committed_chunk_count, batch_count, last_path, resource_budget_json, updated_at_ms, error_message",
    },
    CodeScopeTable {
        table: "software_components",
        columns: "component_id, repository_id, source_scope, ecosystem, name, requirement, resolved_version, dependency_group, source_kind, relationship_state, language_id, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
    CodeScopeTable {
        table: "software_dependency_usages",
        columns: "usage_id, component_id, repository_id, source_scope, ecosystem, package_name, language_id, module, target_hint, resolution_state, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
    CodeScopeTable {
        table: "software_sdk_usages",
        columns: "usage_id, repository_id, source_scope, language_id, module, target_hint, resolution_state, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
    CodeScopeTable {
        table: "software_files",
        columns: "software_file_id, repository_id, source_scope, path, language_id, file_role, parse_status, created_graph_version",
    },
    CodeScopeTable {
        table: "software_topics",
        columns: "topic_id, repository_id, source_scope, name, topic_kind, source_path, line_start, line_end, created_graph_version",
    },
    CodeScopeTable {
        table: "software_relationships",
        columns: "relationship_id, repository_id, source_scope, relationship_kind, source_id, source_kind, target_id, target_kind, target_hint, resolution_state, confidence_basis_points, confidence_tier, evidence_path, evidence_line_start, evidence_line_end, created_graph_version",
    },
    CodeScopeTable {
        table: "software_global_status",
        columns: "source_scope, repository_id, projected_graph_version, stale, component_count, sdk_usage_count, file_count, topic_count, relationship_count, build_target_count, iac_resource_count, design_element_count, projection_schema_version, last_error",
    },
    CodeScopeTable {
        table: "software_build_targets",
        columns: "target_id, repository_id, source_scope, ecosystem, language_id, name, kind, command, output_hint, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
    CodeScopeTable {
        table: "software_iac_resources",
        columns: "resource_id, repository_id, source_scope, language_id, provider, resource_kind, name, scope_hint, target_hint, resolution_state, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
    CodeScopeTable {
        table: "software_design_elements",
        columns: "element_id, repository_id, source_scope, language_id, element_kind, name, parent, summary, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
    },
];

pub(super) fn import_repository_from_database(
    connection: &mut Connection,
    source_path: &Path,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {IMPORT_SCHEMA}"),
        params![source_path.display().to_string()],
    )?;
    let result = import_attached_repository(connection, repository_id, source_scope);
    let detach = connection.execute(&format!("DETACH DATABASE {IMPORT_SCHEMA}"), []);
    match (result, detach) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(StorageError::from(error)),
    }
}

fn import_attached_repository(
    connection: &mut Connection,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    import_repository_metadata(&transaction, repository_id)?;
    if let Some(source_scope) = source_scope {
        import_code_scope(&transaction, repository_id, source_scope)?;
    }
    transaction.commit()?;

    Ok(())
}

fn import_repository_metadata(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
) -> Result<(), StorageError> {
    let main_has_repository = transaction
        .query_row(
            "SELECT 1 FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let copied = transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                   last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                   indexed_file_count, symbol_count, reference_count, chunk_count,
                   stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repositories
            WHERE repository_id = ?1
            "
        ),
        params![repository_id],
    )?;
    if !main_has_repository && copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' is missing from the import database"
        )));
    }
    transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repository_aliases (alias, repository_id)
            SELECT alias, repository_id
            FROM {IMPORT_SCHEMA}.code_repository_aliases
            WHERE repository_id = ?1
            "
        ),
        params![repository_id],
    )?;

    Ok(())
}

fn import_code_scope(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if transaction
        .query_row(
            "SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Ok(());
    }

    delete_scope_index(transaction, source_scope)?;
    let copied = transaction.execute(
        &format!(
            "
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            SELECT source_scope, repository_id, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, indexed_file_count,
                   symbol_count, reference_count, chunk_count, stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repository_scopes
            WHERE source_scope = ?1 AND repository_id = ?2
            "
        ),
        params![source_scope, repository_id],
    )?;
    if copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has no importable source scope '{source_scope}'"
        )));
    }
    for table in CODE_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    for table in IMPORTED_DERIVED_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    backfill_search_metadata_for_scope(transaction, source_scope)?;

    Ok(())
}

pub(super) fn file_fingerprints(
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

pub(super) fn file_fingerprints_for_scope(
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

pub(super) fn apply_snapshot(
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
        delete_path_indexes(
            &transaction,
            &snapshot.source_scope,
            snapshot.files.iter().map(|file| file.path.as_str()),
        )?;
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
    let file_languages_by_path = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
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
    insert_imports_calls_chunks_diagnostics(&transaction, &snapshot)?;
    update_repository_after_snapshot(&transaction, &snapshot)?;
    transaction.commit()?;

    let status = super::code_status::repository_status(connection, &snapshot.repository_id)?
        .ok_or_else(|| {
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
                .saturating_add(snapshot.dependencies.len())
                .saturating_add(snapshot.calls.len())
                .saturating_add(snapshot.feature_flags.len())
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
        SELECT source_scope, tree_hash, path_filters_json, language_filters_json
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
                row.get::<_, String>(1)?,
                parse_json_list(row.get::<_, String>(2)?)?,
                parse_json_list(row.get::<_, String>(3)?)?,
            ))
        },
    )?;
    let mut previous_scope = None;
    for row in rows {
        let (source_scope, tree_hash, stored_path_filters, stored_language_filters) = row?;
        if canonical_path_filters(&stored_path_filters) == requested_path_filters
            && canonical_filter_values(&stored_language_filters) == requested_language_filters
            && (!code_snapshot_scope_is_fact_versioned(&source_scope)
                || code_snapshot_expected_scope_id(
                    &snapshot.repository_id,
                    &tree_hash,
                    &stored_path_filters,
                    &stored_language_filters,
                )
                .is_some_and(|expected| expected == source_scope))
        {
            previous_scope = Some(source_scope);
            break;
        }
    }
    let previous_scope = previous_scope.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' has no matching indexed scope for incremental filters at the current base commit and code fact version",
            snapshot.repository_id
        ))
    })?;
    if previous_scope == snapshot.source_scope {
        return Ok(());
    }
    delete_scope_index(transaction, &snapshot.source_scope)?;
    for table in CODE_SCOPE_TABLES {
        clone_code_table(transaction, table, &previous_scope, &snapshot.source_scope)?;
    }
    backfill_search_metadata_for_scope(transaction, &snapshot.source_scope)?;

    Ok(())
}

fn clone_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &CodeScopeTable,
    previous_scope: &str,
    next_scope: &str,
) -> Result<(), StorageError> {
    let selected_columns = table.columns.replacen("source_scope", "?2", 1);
    transaction.execute(
        &format!(
            "INSERT INTO {table} ({columns}) SELECT {selected_columns} FROM {table} WHERE source_scope = ?1",
            table = table.table,
            columns = table.columns,
        ),
        params![previous_scope, next_scope],
    )?;

    Ok(())
}

fn copy_attached_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &CodeScopeTable,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        &format!(
            "INSERT INTO {table} ({columns}) SELECT {columns} FROM {schema}.{table} WHERE source_scope = ?1",
            table = table.table,
            columns = table.columns,
            schema = IMPORT_SCHEMA,
        ),
        params![source_scope],
    )?;

    Ok(())
}

fn insert_imports_calls_chunks_diagnostics(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    let file_languages_by_path = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let symbol_signatures_by_snapshot_id =
        call_symbol_signatures_by_snapshot_id(transaction, snapshot)?;
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
    super::code_batch::dependencies::insert_dependency_records(
        transaction,
        &snapshot.dependencies,
    )?;
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
    super::code_feature_flags::insert_records(transaction, &snapshot.feature_flags)?;
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
