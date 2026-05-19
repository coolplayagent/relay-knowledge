use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    domain::{CodeIndexBatch, CodeIndexCheckpoint, CodeIndexProgressSummary, CodeIndexSession},
    storage::StorageError,
};

use super::{
    code_cleanup::{count_code_rows, delete_path_indexes, delete_scope_index},
    code_status,
};

#[path = "code_batch/finalize.rs"]
mod finalize;

pub(super) fn begin_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if !session.full_replace {
        return Err(StorageError::InvalidInput(
            "checkpointed code indexing currently requires a full-replace session".to_owned(),
        ));
    }

    let transaction = connection.transaction()?;
    delete_scope_index(&transaction, &session.source_scope)?;
    transaction.execute(
        "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
        params![session.source_scope],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET state = 'indexing', stale = 1, degraded_reason = NULL
        WHERE repository_id = ?1
        ",
        params![session.repository_id],
    )?;
    insert_checkpoint(&transaction, &session, "indexing", None)?;
    transaction.commit()?;

    checkpoint_for_scope(connection, &session.source_scope)
}

pub(super) fn apply_batch(
    connection: &mut Connection,
    batch: CodeIndexBatch,
) -> Result<CodeIndexCheckpoint, StorageError> {
    let transaction = connection.transaction()?;
    delete_path_indexes(
        &transaction,
        &batch.source_scope,
        batch.files.iter().map(|file| file.path.as_str()),
    )?;
    insert_files(&transaction, &batch)?;
    insert_symbols(&transaction, &batch)?;
    let edge_search_languages =
        if should_materialize_intermediate_edge_search(&transaction, &batch)? {
            Some(edge_file_languages_by_path(&transaction, &batch)?)
        } else {
            None
        };
    insert_references(&transaction, &batch, edge_search_languages.as_ref())?;
    insert_imports(&transaction, &batch, edge_search_languages.as_ref())?;
    insert_chunks(&transaction, &batch)?;
    insert_diagnostics(&transaction, &batch)?;
    update_checkpoint_after_batch(&transaction, &batch)?;
    transaction.commit()?;

    checkpoint_for_scope(connection, &batch.source_scope)
}

pub(super) fn finalize_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<crate::domain::CodeIndexSummary, StorageError> {
    let transaction = connection.transaction()?;
    finalize::resolve_scope(&transaction, &session.source_scope, &session.repository_id)?;
    update_repository_after_session(&transaction, &session)?;
    mark_checkpoint_completed(&transaction, &session.source_scope)?;
    transaction.commit()?;

    let status =
        code_status::repository_status(connection, &session.repository_id)?.ok_or_else(|| {
            StorageError::InvalidInput("code repository status is missing after index".to_owned())
        })?;
    let checkpoint = checkpoint_for_scope(connection, &session.source_scope)?;
    let sqlite_write_count = count_scope_rows(connection, &session.source_scope)?;

    Ok(crate::domain::CodeIndexSummary {
        repository_id: session.repository_id,
        source_scope: session.source_scope,
        resolved_commit_sha: session.resolved_commit_sha,
        tree_hash: session.tree_hash,
        indexed_file_count: status.indexed_file_count,
        changed_path_count: session.changed_path_count,
        skipped_unchanged_count: session.skipped_unchanged_count,
        deleted_path_count: session.deleted_paths.len(),
        symbol_count: status.symbol_count,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count: count_scope_diagnostics(
            connection,
            status.last_indexed_scope_id.as_deref(),
        )?,
        progress: CodeIndexProgressSummary {
            git_file_count: session.total_path_count,
            blob_read_count: checkpoint.committed_file_count,
            parsed_file_count: checkpoint.parsed_file_count,
            sqlite_write_count,
            skipped_file_count: session.skipped_unchanged_count,
            degraded_file_count: count_scope_diagnostics(
                connection,
                status.last_indexed_scope_id.as_deref(),
            )?,
            batch_count: checkpoint.batch_count,
            checkpoint_file_count: checkpoint.committed_file_count,
            resource_budget: session.resource_budget,
        },
    })
}

fn insert_checkpoint(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    state: &str,
    error_message: Option<&str>,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        INSERT INTO code_repository_index_checkpoints (
            source_scope, repository_id, state, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, total_path_count,
            parsed_file_count, committed_file_count, committed_symbol_count,
            committed_reference_count, committed_chunk_count, batch_count, last_path,
            resource_budget_json, updated_at_ms, error_message
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, 0, 0, 0, NULL, ?9, ?10, ?11)
        ",
        params![
            session.source_scope,
            session.repository_id,
            state,
            session.resolved_commit_sha,
            session.tree_hash,
            json(&session.path_filters)?,
            json(&session.language_filters)?,
            session.total_path_count,
            json(&session.resource_budget)?,
            now_millis(),
            error_message,
        ],
    )?;

    Ok(())
}

fn insert_files(transaction: &Transaction<'_>, batch: &CodeIndexBatch) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
            line_count, parse_status, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
    )?;
    for file in &batch.files {
        statement.execute(params![
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
        ])?;
    }

    Ok(())
}

fn insert_symbols(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name,
            qualified_name, kind, signature, doc_comment, byte_start, byte_end,
            line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    let mut search_documents = super::SearchDocumentInserter::new(transaction)?;
    for symbol in &batch.symbols {
        statement.execute(params![
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
        ])?;
        search_documents.insert(
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

    Ok(())
}

fn insert_references(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
    file_languages_by_path: Option<&BTreeMap<String, String>>,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
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
    let mut search_documents = if file_languages_by_path.is_some() {
        Some(super::SearchDocumentInserter::new(transaction)?)
    } else {
        None
    };
    for reference in &batch.references {
        statement.execute(params![
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
        if let (Some(search_documents), Some(file_languages_by_path)) =
            (search_documents.as_mut(), file_languages_by_path)
        {
            search_documents.insert(
                &reference.source_scope,
                "reference",
                &reference.reference_id,
                &reference.path,
                file_languages_by_path
                    .get(reference.path.as_str())
                    .map(String::as_str)
                    .unwrap_or_default(),
                [
                    reference.name.as_str(),
                    reference.kind.as_str(),
                    reference.target_hint.as_deref().unwrap_or_default(),
                    reference.path.as_str(),
                ],
            )?;
        }
    }

    Ok(())
}

fn insert_imports(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
    file_languages_by_path: Option<&BTreeMap<String, String>>,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_imports (
            repository_id, source_scope, import_id, file_id, path, module, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;
    let mut search_documents = if file_languages_by_path.is_some() {
        Some(super::SearchDocumentInserter::new(transaction)?)
    } else {
        None
    };
    for import in &batch.imports {
        statement.execute(params![
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
        if let (Some(search_documents), Some(file_languages_by_path)) =
            (search_documents.as_mut(), file_languages_by_path)
        {
            search_documents.insert(
                &import.source_scope,
                "import",
                &import.import_id,
                &import.path,
                file_languages_by_path
                    .get(import.path.as_str())
                    .map(String::as_str)
                    .unwrap_or_default(),
                [
                    import.module.as_str(),
                    import.target_hint.as_deref().unwrap_or_default(),
                    import.path.as_str(),
                ],
            )?;
        }
    }

    Ok(())
}

fn insert_chunks(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_chunks (
            repository_id, source_scope, chunk_id, file_id, path, language_id, content,
            byte_start, byte_end, line_start, line_end, symbol_snapshot_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;
    let mut search_documents = super::SearchDocumentInserter::new(transaction)?;
    for chunk in &batch.chunks {
        statement.execute(params![
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
        search_documents.insert(
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

    Ok(())
}

fn should_materialize_intermediate_edge_search(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<bool, StorageError> {
    if batch.references.is_empty() && batch.imports.is_empty() {
        return Ok(false);
    }

    let active_scope = transaction
        .query_row(
            "
            SELECT last_indexed_scope_id
            FROM code_repositories
            WHERE repository_id = ?1
            ",
            params![batch.repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();

    if active_scope.as_deref() == Some(batch.source_scope.as_str()) {
        return Ok(true);
    }

    transaction
        .query_row(
            "
            SELECT 1
            FROM code_repository_scopes
            WHERE source_scope = ?1
              AND repository_id = ?2
            ",
            params![batch.source_scope, batch.repository_id],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

fn edge_file_languages_by_path(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<BTreeMap<String, String>, StorageError> {
    if batch.references.is_empty() && batch.imports.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut languages = batch
        .files
        .iter()
        .map(|file| (file.path.clone(), file.language_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let missing_paths = edge_paths_missing_from_batch(batch, &languages);
    if missing_paths.is_empty() {
        return Ok(languages);
    }

    let mut statement = transaction.prepare(
        "
        SELECT language_id
        FROM code_repository_files
        WHERE source_scope = ?1 AND path = ?2
        ",
    )?;
    for path in missing_paths {
        if let Some(language_id) = statement
            .query_row(params![batch.source_scope.as_str(), path.as_str()], |row| {
                row.get(0)
            })
            .optional()?
        {
            languages.insert(path, language_id);
        }
    }

    Ok(languages)
}

fn edge_paths_missing_from_batch(
    batch: &CodeIndexBatch,
    languages: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut missing_paths = Vec::<String>::new();
    for path in batch
        .references
        .iter()
        .map(|reference| reference.path.as_str())
        .chain(batch.imports.iter().map(|import| import.path.as_str()))
    {
        if !languages.contains_key(path)
            && !missing_paths.iter().any(|known| known.as_str() == path)
        {
            missing_paths.push(path.to_owned());
        }
    }

    missing_paths
}

fn insert_diagnostics(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_file_diagnostics
            (repository_id, source_scope, path, parse_status, message)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
    )?;
    for diagnostic in &batch.diagnostics {
        statement.execute(params![
            diagnostic.repository_id,
            diagnostic.source_scope,
            diagnostic.path,
            diagnostic.parse_status.as_str(),
            diagnostic.message,
        ])?;
    }

    Ok(())
}

fn update_checkpoint_after_batch(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let batch_is_new = checkpoint_batch_is_new(transaction, batch)?;
    let delta_files = if batch_is_new { batch.files.len() } else { 0 };
    let delta_symbols = if batch_is_new { batch.symbols.len() } else { 0 };
    let delta_references = if batch_is_new {
        batch.references.len()
    } else {
        0
    };
    let delta_chunks = if batch_is_new { batch.chunks.len() } else { 0 };
    let delta_batches = usize::from(batch_is_new);
    transaction.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET parsed_file_count = parsed_file_count + ?2,
            committed_file_count = committed_file_count + ?3,
            committed_symbol_count = committed_symbol_count + ?4,
            committed_reference_count = committed_reference_count + ?5,
            committed_chunk_count = committed_chunk_count + ?6,
            batch_count = batch_count + ?7,
            last_path = COALESCE(?8, last_path),
            updated_at_ms = ?9
        WHERE source_scope = ?1
        ",
        params![
            batch.source_scope,
            delta_files,
            delta_files,
            delta_symbols,
            delta_references,
            delta_chunks,
            delta_batches,
            batch.files.last().map(|file| file.path.as_str()),
            now_millis(),
        ],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET state = 'indexing',
            indexed_file_count = (
                SELECT committed_file_count
                FROM code_repository_index_checkpoints
                WHERE source_scope = ?2
            ),
            symbol_count = (
                SELECT committed_symbol_count
                FROM code_repository_index_checkpoints
                WHERE source_scope = ?2
            ),
            reference_count = (
                SELECT committed_reference_count
                FROM code_repository_index_checkpoints
                WHERE source_scope = ?2
            ),
            chunk_count = (
                SELECT committed_chunk_count
                FROM code_repository_index_checkpoints
                WHERE source_scope = ?2
            ),
            stale = 1
        WHERE repository_id = ?1
        ",
        params![batch.repository_id, batch.source_scope],
    )?;

    Ok(())
}

fn checkpoint_batch_is_new(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "
            SELECT batch_count
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![batch.source_scope],
            |row| row.get::<_, usize>(0),
        )
        .map(|batch_count| batch.batch_index > batch_count)
        .map_err(StorageError::from)
}

fn update_repository_after_session(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<(), StorageError> {
    for tombstone in &session.tombstones {
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
    let file_count = count_code_rows(transaction, "code_repository_files", &session.source_scope)?;
    let symbol_count = count_code_rows(
        transaction,
        "code_repository_symbols",
        &session.source_scope,
    )?;
    let reference_count = count_code_rows(
        transaction,
        "code_repository_references",
        &session.source_scope,
    )?;
    let chunk_count =
        count_code_rows(transaction, "code_repository_chunks", &session.source_scope)?;
    let degraded_file_count = count_code_rows(
        transaction,
        "code_repository_file_diagnostics",
        &session.source_scope,
    )?;
    let degraded_reason = (degraded_file_count > 0)
        .then(|| format!("{degraded_file_count} file(s) degraded during code indexing"));
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
            session.source_scope,
            session.repository_id,
            session.resolved_commit_sha,
            session.tree_hash,
            json(&session.path_filters)?,
            json(&session.language_filters)?,
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
            session.repository_id,
            session.source_scope,
            session.resolved_commit_sha,
            session.tree_hash,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;

    Ok(())
}

fn mark_checkpoint_completed(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = 'completed', updated_at_ms = ?2, error_message = NULL
        WHERE source_scope = ?1
        ",
        params![source_scope, now_millis()],
    )?;

    Ok(())
}

fn checkpoint_for_scope(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<CodeIndexCheckpoint, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id, source_scope, state, total_path_count, parsed_file_count,
                   committed_file_count, committed_symbol_count, committed_reference_count,
                   committed_chunk_count, batch_count, last_path, resource_budget_json
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| {
                let resource_budget = serde_json::from_str(row.get::<_, String>(11)?.as_str())
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                Ok(CodeIndexCheckpoint {
                    repository_id: row.get(0)?,
                    source_scope: row.get(1)?,
                    state: row.get(2)?,
                    total_path_count: row.get(3)?,
                    parsed_file_count: row.get(4)?,
                    committed_file_count: row.get(5)?,
                    committed_symbol_count: row.get(6)?,
                    committed_reference_count: row.get(7)?,
                    committed_chunk_count: row.get(8)?,
                    batch_count: row.get(9)?,
                    last_path: row.get(10)?,
                    resource_budget,
                })
            },
        )
        .map_err(StorageError::from)
}

fn count_scope_rows(connection: &Connection, source_scope: &str) -> Result<usize, StorageError> {
    let mut total = 0usize;
    for table in [
        "code_repository_files",
        "code_repository_symbols",
        "code_repository_references",
        "code_repository_imports",
        "code_repository_calls",
        "code_repository_chunks",
        "code_repository_file_diagnostics",
    ] {
        let count = connection.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
            |row| row.get::<_, usize>(0),
        )?;
        total = total.saturating_add(count);
    }

    Ok(total)
}

fn count_scope_diagnostics(
    connection: &Connection,
    source_scope: Option<&str>,
) -> Result<usize, StorageError> {
    let Some(source_scope) = source_scope else {
        return Ok(0);
    };
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_file_diagnostics
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
