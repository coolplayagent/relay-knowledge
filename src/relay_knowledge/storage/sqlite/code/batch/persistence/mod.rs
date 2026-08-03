//! Applies replay-safe, resource-bounded code fact batches to SQLite.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::super::{
    SearchDocumentInserter,
    cleanup::{delete_path_indexes, path_indexes_exist},
    feature_flags, routes, symbols,
};
use super::{checkpoint, dependencies};
use crate::{
    domain::{CodeIndexBatch, CodeIndexCheckpoint},
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(in super::super) fn apply_batch(
    connection: &mut Connection,
    batch: CodeIndexBatch,
) -> Result<CodeIndexCheckpoint, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        apply_batch_once(connection, &batch)
    })
}

fn apply_batch_once(
    connection: &mut Connection,
    batch: &CodeIndexBatch,
) -> Result<CodeIndexCheckpoint, StorageError> {
    prepare_batch_staging(connection, batch)?;
    let transaction = connection.transaction()?;
    let batch_is_new = checkpoint_batch_is_new(&transaction, batch)?;
    delete_batch_path_indexes_if_needed(&transaction, batch, batch_is_new)?;
    insert_files(&transaction, batch)?;
    symbols::insert_records(&transaction, &batch.symbols)?;
    let materialize_edge_search = should_materialize_intermediate_edge_search(&transaction, batch)?;
    let edge_search_languages = if materialize_edge_search {
        Some(edge_file_languages_by_path(&transaction, batch)?)
    } else {
        None
    };
    insert_references(&transaction, batch, edge_search_languages.as_ref())?;
    insert_imports(&transaction, batch, edge_search_languages.as_ref())?;
    dependencies::insert_dependencies(&transaction, batch)?;
    feature_flags::insert_records(&transaction, &batch.feature_flags)?;
    routes::insert_records(&transaction, &batch.routes)?;
    insert_chunks(&transaction, batch)?;
    insert_diagnostics(&transaction, batch)?;
    update_checkpoint_after_batch(&transaction, batch, batch_is_new)?;
    mark_batch_staging_published(&transaction, batch)?;
    transaction.commit()?;

    checkpoint::load(connection, &batch.source_scope)
}

/// Commits a durable batch manifest before fact publication so a crash between
/// staging and publish remains observable and replayable without a second writer.
fn prepare_batch_staging(
    connection: &mut Connection,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let fact_row_count = batch.files.len()
        + batch.symbols.len()
        + batch.references.len()
        + batch.imports.len()
        + batch.dependencies.len()
        + batch.feature_flags.len()
        + batch.routes.len()
        + batch.chunks.len()
        + batch.diagnostics.len();
    let now = checkpoint::now_millis();
    let transaction = connection.transaction()?;
    transaction.execute(
        "
        INSERT INTO code_repository_index_batch_staging
            (source_scope, batch_index, state, file_count, fact_row_count,
             created_at_ms, updated_at_ms)
        VALUES (?1, ?2, 'staged', ?3, ?4, ?5, ?5)
        ON CONFLICT(source_scope, batch_index) DO UPDATE SET
            state = 'staged',
            file_count = excluded.file_count,
            fact_row_count = excluded.fact_row_count,
            updated_at_ms = excluded.updated_at_ms
        ",
        rusqlite::params![
            batch.source_scope,
            batch.batch_index,
            batch.files.len(),
            fact_row_count,
            now,
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn mark_batch_staging_published(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_index_batch_staging
        SET state = 'published', updated_at_ms = ?3
        WHERE source_scope = ?1 AND batch_index = ?2
        ",
        rusqlite::params![
            batch.source_scope,
            batch.batch_index,
            checkpoint::now_millis()
        ],
    )?;
    Ok(())
}

fn delete_batch_path_indexes_if_needed(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
    batch_is_new: bool,
) -> Result<(), StorageError> {
    let paths = batch
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(());
    }

    let should_delete = !batch_is_new
        || (batch.batch_index > 1
            && path_indexes_exist(transaction, &batch.source_scope, paths.iter().copied())?);
    if should_delete {
        delete_path_indexes(transaction, &batch.source_scope, paths)?;
    }

    Ok(())
}

fn insert_files(transaction: &Transaction<'_>, batch: &CodeIndexBatch) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len,
            line_count, parse_status, is_generated, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
            file.is_generated,
            file.degraded_reason,
        ])?;
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
        Some(SearchDocumentInserter::new(transaction)?)
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
    if let Some(search_documents) = search_documents {
        search_documents.finish()?;
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
        Some(SearchDocumentInserter::new(transaction)?)
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
    if let Some(search_documents) = search_documents {
        search_documents.finish()?;
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
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
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
    search_documents.finish()?;

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
    batch_is_new: bool,
) -> Result<(), StorageError> {
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
            checkpoint::now_millis(),
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
