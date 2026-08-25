//! Applies replay-safe, resource-bounded code fact batches to SQLite.

use std::{borrow::Cow, collections::BTreeMap, sync::OnceLock};

use rusqlite::{
    Connection, OptionalExtension, ToSql, Transaction, limits::Limit, params, params_from_iter,
};

use super::super::{
    SearchDocumentInserter,
    cleanup::{delete_path_indexes, path_indexes_exist},
    feature_flags, routes, symbols,
};
use super::{checkpoint, dependencies};
use crate::{
    domain::{
        CodeIndexBatch, CodeIndexCheckpoint, RepositoryCodeChunkRecord,
        RepositoryCodeReferenceRecord,
    },
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reference_bulk_tests.rs"]
mod reference_bulk_tests;

#[cfg(test)]
#[path = "chunk_bulk_tests.rs"]
mod chunk_bulk_tests;

const REFERENCE_INSERT_BATCH_SIZE: usize = 1_024;
const REFERENCE_INSERT_COLUMN_COUNT: usize = 16;
const REFERENCE_INSERT_BIND_COUNT: usize =
    REFERENCE_INSERT_BATCH_SIZE * REFERENCE_INSERT_COLUMN_COUNT;
const REFERENCE_INSERT_ROW_PLACEHOLDERS: &str = "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
static REFERENCE_INSERT_FULL_SQL: OnceLock<String> = OnceLock::new();
const _: () = assert!(REFERENCE_INSERT_BIND_COUNT == 16_384);
const CHUNK_INSERT_BATCH_SIZE: usize = 1_024;
const CHUNK_INSERT_COLUMN_COUNT: usize = 12;
const CHUNK_INSERT_BIND_COUNT: usize = CHUNK_INSERT_BATCH_SIZE * CHUNK_INSERT_COLUMN_COUNT;
const CHUNK_INSERT_ROW_PLACEHOLDERS: &str = "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
static CHUNK_INSERT_FULL_SQL: OnceLock<String> = OnceLock::new();
const _: () = assert!(CHUNK_INSERT_BIND_COUNT == 12_288);

pub(in super::super) fn apply_batch(
    connection: &mut Connection,
    batch: CodeIndexBatch,
) -> Result<CodeIndexCheckpoint, StorageError> {
    apply_batch_with_fence(connection, batch, None)
}

pub(in super::super) fn apply_batch_with_fence(
    connection: &mut Connection,
    batch: CodeIndexBatch,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&batch.repository_id)?;
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        apply_batch_once(connection, &batch, fence)
    })
}

fn apply_batch_once(
    connection: &mut Connection,
    batch: &CodeIndexBatch,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if !prepare_batch_staging(connection, batch, fence)? {
        return checkpoint::load(connection, &batch.source_scope);
    }
    let transaction = connection.transaction()?;
    if fence.is_none() {
        super::super::tasks::enforce_unfenced_target(
            &transaction,
            &batch.repository_id,
            &batch.source_scope,
        )?;
    }
    let batch_is_new = checkpoint_batch_is_new(&transaction, batch)?;
    if !batch_is_new {
        if let Some(fence) = fence {
            fence.validate_target_scope(&transaction, &batch.source_scope)?;
            fence.validate(&transaction)?;
        }
        transaction.commit()?;
        return checkpoint::load(connection, &batch.source_scope);
    }
    delete_batch_path_indexes_if_needed(&transaction, batch)?;
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
    update_checkpoint_after_batch(&transaction, batch)?;
    mark_batch_staging_published(&transaction, batch)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &batch.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    checkpoint::load(connection, &batch.source_scope)
}

/// Commits a durable batch manifest before fact publication so a crash between
/// staging and publish remains observable and replayable without a second writer.
fn prepare_batch_staging(
    connection: &mut Connection,
    batch: &CodeIndexBatch,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<bool, StorageError> {
    let fact_row_count = checked_fact_row_count(batch)?;
    let now = checkpoint::now_millis();
    let transaction = connection.transaction()?;
    if fence.is_none() {
        super::super::tasks::enforce_unfenced_target(
            &transaction,
            &batch.repository_id,
            &batch.source_scope,
        )?;
    }
    let batch_is_new = checkpoint_batch_is_new(&transaction, batch)?;
    if !batch_is_new {
        if let Some(fence) = fence {
            fence.validate_target_scope(&transaction, &batch.source_scope)?;
            fence.validate(&transaction)?;
        }
        transaction.commit()?;
        return Ok(false);
    }
    let changed = transaction.execute(
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
        WHERE code_repository_index_batch_staging.state = 'staged'
        ",
        rusqlite::params![
            batch.source_scope,
            batch.batch_index,
            batch.files.len(),
            fact_row_count,
            now,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "code index batch {} for scope '{}' could not prepare exactly one staged manifest",
            batch.batch_index, batch.source_scope
        )));
    }
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &batch.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;
    Ok(true)
}

fn mark_batch_staging_published(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let changed = transaction.execute(
        "
        UPDATE code_repository_index_batch_staging
        SET state = 'published', updated_at_ms = ?3
        WHERE source_scope = ?1 AND batch_index = ?2 AND state = 'staged'
        ",
        rusqlite::params![
            batch.source_scope,
            batch.batch_index,
            checkpoint::now_millis()
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "code index batch {} for scope '{}' could not publish exactly one staged manifest",
            batch.batch_index, batch.source_scope
        )));
    }
    Ok(())
}

fn delete_batch_path_indexes_if_needed(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    let paths = batch
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(());
    }

    let should_delete = batch.batch_index > 1
        && path_indexes_exist(transaction, &batch.source_scope, paths.iter().copied())?;
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
    _file_languages_by_path: Option<&BTreeMap<String, String>>,
) -> Result<(), StorageError> {
    let reference_rows_per_statement = if batch.references.is_empty() {
        REFERENCE_INSERT_BATCH_SIZE
    } else {
        let variable_limit = usize::try_from(
            transaction.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER),
        )
        .map_err(|_| {
            StorageError::Invariant(
                "SQLite reported a negative variable limit for reference persistence".to_owned(),
            )
        })?;
        let rows_within_variable_limit = variable_limit / REFERENCE_INSERT_COLUMN_COUNT;
        if rows_within_variable_limit == 0 {
            return Err(StorageError::Invariant(format!(
                "SQLite variable limit {variable_limit} cannot admit one {}-column reference row",
                REFERENCE_INSERT_COLUMN_COUNT
            )));
        }
        REFERENCE_INSERT_BATCH_SIZE.min(rows_within_variable_limit)
    };
    let mut full_groups = batch.references.chunks_exact(reference_rows_per_statement);
    if batch.references.len() >= reference_rows_per_statement {
        let full_sql: Cow<'static, str> =
            if reference_rows_per_statement == REFERENCE_INSERT_BATCH_SIZE {
                Cow::Borrowed(
                    REFERENCE_INSERT_FULL_SQL
                        .get_or_init(|| reference_insert_sql(REFERENCE_INSERT_BATCH_SIZE)),
                )
            } else {
                Cow::Owned(reference_insert_sql(reference_rows_per_statement))
            };
        let mut statement = transaction.prepare_cached(full_sql.as_ref())?;
        for references in full_groups.by_ref() {
            statement.execute(params_from_iter(reference_insert_parameters(references)))?;
        }
    }
    let tail = full_groups.remainder();
    if !tail.is_empty() {
        let tail_sql = reference_insert_sql(tail.len());
        let mut statement = transaction.prepare(&tail_sql)?;
        statement.execute(params_from_iter(reference_insert_parameters(tail)))?;
    }
    Ok(())
}

fn reference_insert_sql(row_count: usize) -> String {
    let placeholders = std::iter::repeat_n(REFERENCE_INSERT_ROW_PLACEHOLDERS, row_count)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "
        INSERT INTO code_repository_references (
            repository_id, source_scope, reference_id, file_id, path, name, kind,
            target_symbol_snapshot_id, target_hint, resolution_state,
            confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end
        )
        VALUES {placeholders}
        "
    )
}

fn reference_insert_parameters(references: &[RepositoryCodeReferenceRecord]) -> Vec<&dyn ToSql> {
    let mut parameters = Vec::with_capacity(references.len() * REFERENCE_INSERT_COLUMN_COUNT);
    for reference in references {
        parameters.extend([
            &reference.repository_id as &dyn ToSql,
            &reference.source_scope,
            &reference.reference_id,
            &reference.file_id,
            &reference.path,
            &reference.name,
            &reference.kind,
            &reference.target_symbol_snapshot_id,
            &reference.target_hint,
            &reference.resolution_state,
            &reference.confidence_basis_points,
            &reference.confidence_tier,
            &reference.byte_range.start,
            &reference.byte_range.end,
            &reference.line_range.start,
            &reference.line_range.end,
        ]);
    }
    parameters
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
    insert_chunk_facts(transaction, &batch.chunks)?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for chunk in &batch.chunks {
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

fn insert_chunk_facts(
    transaction: &Transaction<'_>,
    chunks: &[RepositoryCodeChunkRecord],
) -> Result<(), StorageError> {
    if chunks.is_empty() {
        return Ok(());
    }
    let variable_limit = usize::try_from(transaction.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER))
        .map_err(|_| {
            StorageError::Invariant(
                "SQLite reported a negative variable limit for chunk persistence".to_owned(),
            )
        })?;
    let rows_within_variable_limit = variable_limit / CHUNK_INSERT_COLUMN_COUNT;
    let rows_per_statement = CHUNK_INSERT_BATCH_SIZE.min(rows_within_variable_limit);
    if rows_per_statement == 0 {
        return Err(StorageError::Invariant(format!(
            "SQLite variable limit {variable_limit} cannot admit one {}-column chunk row",
            CHUNK_INSERT_COLUMN_COUNT
        )));
    }

    let mut full_groups = chunks.chunks_exact(rows_per_statement);
    if chunks.len() >= rows_per_statement {
        let full_sql: Cow<'static, str> = if rows_per_statement == CHUNK_INSERT_BATCH_SIZE {
            Cow::Borrowed(
                CHUNK_INSERT_FULL_SQL.get_or_init(|| chunk_insert_sql(CHUNK_INSERT_BATCH_SIZE)),
            )
        } else {
            Cow::Owned(chunk_insert_sql(rows_per_statement))
        };
        let mut statement = transaction.prepare_cached(full_sql.as_ref())?;
        for group in full_groups.by_ref() {
            statement.execute(params_from_iter(chunk_insert_parameters(group)))?;
        }
    }
    let tail = full_groups.remainder();
    if !tail.is_empty() {
        let tail_sql = chunk_insert_sql(tail.len());
        let mut statement = transaction.prepare(&tail_sql)?;
        statement.execute(params_from_iter(chunk_insert_parameters(tail)))?;
    }

    Ok(())
}

fn chunk_insert_sql(row_count: usize) -> String {
    let placeholders = std::iter::repeat_n(CHUNK_INSERT_ROW_PLACEHOLDERS, row_count)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "
        INSERT INTO code_repository_chunks (
            repository_id, source_scope, chunk_id, file_id, path, language_id, content,
            byte_start, byte_end, line_start, line_end, symbol_snapshot_id
        )
        VALUES {placeholders}
        "
    )
}

fn chunk_insert_parameters(chunks: &[RepositoryCodeChunkRecord]) -> Vec<&dyn ToSql> {
    let mut parameters = Vec::with_capacity(chunks.len() * CHUNK_INSERT_COLUMN_COUNT);
    for chunk in chunks {
        parameters.extend([
            &chunk.repository_id as &dyn ToSql,
            &chunk.source_scope,
            &chunk.chunk_id,
            &chunk.file_id,
            &chunk.path,
            &chunk.language_id,
            &chunk.content,
            &chunk.byte_range.start,
            &chunk.byte_range.end,
            &chunk.line_range.start,
            &chunk.line_range.end,
            &chunk.symbol_snapshot_id,
        ]);
    }
    parameters
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

    // A scope registry row can describe a stale staged or retained generation.
    // Only the active scope requires interim edge-search continuity during a batch.
    Ok(active_scope.as_deref() == Some(batch.source_scope.as_str()))
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
    let fact_row_count = checked_fact_row_count(batch)?;
    let staged_fact_row_count = transaction
        .query_row(
            "SELECT fact_row_count
             FROM code_repository_index_batch_staging
             WHERE source_scope = ?1 AND batch_index = ?2 AND state = 'staged'",
            params![batch.source_scope, batch.batch_index],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)?;
    if staged_fact_row_count != fact_row_count {
        return Err(StorageError::Invariant(format!(
            "code index batch {} for scope '{}' changed its staged fact-row proof",
            batch.batch_index, batch.source_scope
        )));
    }
    let previous_batch_count = batch.batch_index.checked_sub(1).ok_or_else(|| {
        StorageError::Invariant(format!(
            "code index batch 0 for scope '{}' cannot advance checkpoint progress",
            batch.source_scope
        ))
    })?;
    let changed = transaction.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET parsed_file_count = parsed_file_count + ?2,
            committed_file_count = committed_file_count + ?3,
            committed_symbol_count = committed_symbol_count + ?4,
            committed_reference_count = committed_reference_count + ?5,
            committed_chunk_count = committed_chunk_count + ?6,
            committed_fact_row_count = CASE
                WHEN batch_count = 0 THEN ?7
                WHEN committed_fact_row_count = 0 THEN 0
                ELSE committed_fact_row_count + ?7
            END,
            batch_count = batch_count + ?8,
            last_path = CASE
                WHEN ?8 > 0 THEN COALESCE(?9, last_path)
                ELSE last_path
            END,
            updated_at_ms = ?10
        WHERE source_scope = ?1 AND state = 'indexing' AND batch_count = ?11
        ",
        params![
            batch.source_scope,
            batch.files.len(),
            batch.files.len(),
            batch.symbols.len(),
            batch.references.len(),
            batch.chunks.len(),
            staged_fact_row_count,
            1_usize,
            batch.files.last().map(|file| file.path.as_str()),
            checkpoint::now_millis(),
            previous_batch_count,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "code index batch {} for scope '{}' could not advance exactly one indexing checkpoint",
            batch.batch_index, batch.source_scope
        )));
    }
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

fn checked_fact_row_count(batch: &CodeIndexBatch) -> Result<usize, StorageError> {
    [
        batch.files.len(),
        batch.symbols.len(),
        batch.references.len(),
        batch.imports.len(),
        batch.dependencies.len(),
        batch.feature_flags.len(),
        batch.routes.len(),
        batch.chunks.len(),
        batch.diagnostics.len(),
    ]
    .into_iter()
    .try_fold(0usize, |total, count| {
        total.checked_add(count).ok_or_else(|| {
            StorageError::CapacityExceeded(format!(
                "code index batch {} for scope '{}' exceeds the fact-row counter",
                batch.batch_index, batch.source_scope
            ))
        })
    })
}

fn checkpoint_batch_is_new(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<bool, StorageError> {
    let (state, batch_count) = transaction
        .query_row(
            "
            SELECT state, batch_count
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![batch.source_scope],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)),
        )
        .map_err(StorageError::from)?;
    if batch.batch_index > 0 && batch.batch_index <= batch_count {
        return Ok(false);
    }
    if state != "indexing" {
        return Err(StorageError::Invariant(format!(
            "code index checkpoint '{}' in state '{}' no longer accepts new batch {}",
            batch.source_scope, state, batch.batch_index
        )));
    }
    let next_batch = batch_count.checked_add(1).ok_or_else(|| {
        StorageError::Invariant(format!(
            "code index checkpoint '{}' batch count cannot advance",
            batch.source_scope
        ))
    })?;
    if batch.batch_index == next_batch {
        return Ok(true);
    }

    Err(StorageError::Invariant(format!(
        "code index batch {} for scope '{}' must be the next batch {} or a replay through {}",
        batch.batch_index, batch.source_scope, next_batch, batch_count
    )))
}
