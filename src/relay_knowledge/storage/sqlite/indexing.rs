use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

mod cursor_metadata;
mod task_queue;

use cursor_metadata::{
    CursorBackendMetadata, CursorBackendMetadataRequest, checked_model_dimension,
    cursor_backend_metadata, cursor_indexed_graph_version,
};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality, IndexState, IndexStatus},
    storage::{
        DEFAULT_INDEX_SOURCE_SCOPE, IndexCursor, IndexLag, IndexRefreshClaimRequest,
        IndexRefreshCompletion, IndexRefreshDiagnostics, IndexRefreshFailure,
        IndexRefreshQueueRequest, IndexRefreshTask, IndexRefreshTaskState, StorageError,
    },
};

pub(super) const DEFAULT_SCOPE: &str = DEFAULT_INDEX_SOURCE_SCOPE;
const TEXT_MODALITY: IndexModality = IndexModality::TEXT;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS index_status (
            kind TEXT PRIMARY KEY,
            index_version INTEGER NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS index_cursors (
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            modality TEXT NOT NULL,
            index_version INTEGER NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_error TEXT,
            PRIMARY KEY (kind, source_scope, modality)
        );

        CREATE TABLE IF NOT EXISTS index_scope_manifest (
            source_scope TEXT PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS index_refresh_tasks (
            task_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            modality TEXT NOT NULL,
            target_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            cursor_before INTEGER NOT NULL,
            cursor_after INTEGER,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        ",
    )?;
    super::ensure_column(
        connection,
        "graph_mutations",
        "affected_scopes_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    super::ensure_column(
        connection,
        "graph_mutations",
        "affected_entity_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    super::ensure_column(
        connection,
        "graph_mutations",
        "evidence_ids_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    super::ensure_column(
        connection,
        "graph_mutations",
        "source_hashes_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    super::ensure_column(connection, "index_cursors", "source_hash", "TEXT")?;
    super::ensure_column(connection, "index_cursors", "backend_cursor", "TEXT")?;
    super::ensure_column(connection, "index_cursors", "model_name", "TEXT")?;
    super::ensure_column(connection, "index_cursors", "model_dimension", "INTEGER")?;

    for kind in IndexKind::ALL {
        connection.execute(
            "INSERT OR IGNORE INTO index_status
             (kind, index_version, indexed_graph_version, state, last_error)
             VALUES (?1, 0, 0, 'fresh', NULL)",
            params![kind.as_str()],
        )?;
    }
    connection.execute(
        "
        INSERT OR IGNORE INTO index_scope_manifest (source_scope)
        SELECT DISTINCT source_scope FROM evidence
        ",
        [],
    )?;
    connection.execute(
        "
        INSERT OR IGNORE INTO index_scope_manifest (source_scope)
        SELECT DISTINCT source_scope FROM index_cursors
        ",
        [],
    )?;

    Ok(())
}

pub(super) fn mark_mutation_cursors_stale(
    transaction: &Transaction<'_>,
    scopes: &[String],
) -> Result<(), StorageError> {
    for scope in scopes {
        transaction.execute(
            "INSERT OR IGNORE INTO index_scope_manifest (source_scope) VALUES (?1)",
            params![scope],
        )?;
        for kind in IndexKind::ALL {
            transaction.execute(
                "
                INSERT OR IGNORE INTO index_cursors (
                    kind, source_scope, modality, index_version,
                    indexed_graph_version, state, last_error
                )
                VALUES (?1, ?2, ?3, 0, 0, 'stale', NULL)
                ",
                params![kind.as_str(), scope, TEXT_MODALITY.as_str()],
            )?;
            transaction.execute(
                "
                UPDATE index_cursors
                SET state = 'stale', last_error = NULL
                WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
                ",
                params![kind.as_str(), scope, TEXT_MODALITY.as_str()],
            )?;
        }
    }

    Ok(())
}

pub(super) fn index_statuses(connection: &Connection) -> Result<Vec<IndexStatus>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, index_version, indexed_graph_version, state, last_error
        FROM index_status
        ORDER BY kind ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        let state: String = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u64>(1)?,
            row.get::<_, u64>(2)?,
            state,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let raw_statuses = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let statuses = raw_statuses
        .into_iter()
        .map(
            |(kind, index_version, indexed_graph_version, state, last_error)| {
                let mut status = IndexStatus {
                    kind: parse_index_kind(&kind)?,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: parse_index_state(&state)?,
                    last_error,
                };
                apply_cursor_integrity(connection, &mut status)?;

                Ok(status)
            },
        )
        .collect::<Result<Vec<_>, StorageError>>()?;
    validate_required_index_statuses(&statuses)?;

    Ok(statuses)
}

pub(super) fn mark_refresh_complete(
    connection: &mut Connection,
    kind: IndexKind,
    graph_version: GraphVersion,
) -> Result<IndexStatus, StorageError> {
    let Some(current) = read_index_status(connection, kind)? else {
        return Err(StorageError::InvalidInput(format!(
            "index status row for '{}' is missing",
            kind.as_str()
        )));
    };
    if current.indexed_graph_version >= graph_version && current.state == IndexState::Fresh {
        return Ok(current);
    }

    connection.execute(
        "
        INSERT OR IGNORE INTO index_cursors (
            kind, source_scope, modality, index_version,
            indexed_graph_version, state, last_error
        )
        VALUES (?1, ?2, ?3, 0, 0, 'stale', NULL)
        ",
        params![kind.as_str(), DEFAULT_SCOPE, TEXT_MODALITY.as_str()],
    )?;
    let cursor_before =
        cursor_indexed_graph_version(connection, kind, DEFAULT_SCOPE, TEXT_MODALITY)?
            .unwrap_or(GraphVersion::ZERO);
    let metadata = cursor_backend_metadata(
        connection,
        CursorBackendMetadataRequest {
            kind,
            scope: DEFAULT_SCOPE,
            modality: TEXT_MODALITY,
            cursor_before,
            graph_version,
            model_name: None,
            model_dimension: None,
        },
    )?;
    mark_cursor_complete(
        connection,
        kind,
        DEFAULT_SCOPE,
        TEXT_MODALITY,
        graph_version,
        None,
        &metadata,
    )?;
    recompute_aggregate_status(connection, kind, graph_version)?;

    read_index_status(connection, kind)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "index status row for '{}' is missing",
            kind.as_str()
        ))
    })
}

pub(super) fn index_cursors(connection: &mut Connection) -> Result<Vec<IndexCursor>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, source_scope, modality, index_version,
               indexed_graph_version, state, last_error,
               source_hash, backend_cursor, model_name, model_dimension
        FROM index_cursors
        ORDER BY kind ASC, source_scope ASC, modality ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, u64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<u64>>(10)?,
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(
            |(
                kind,
                source_scope,
                modality,
                index_version,
                indexed_graph_version,
                state,
                last_error,
                source_hash,
                backend_cursor,
                model_name,
                model_dimension,
            )| {
                Ok(IndexCursor {
                    kind: parse_index_kind(&kind)?,
                    source_scope,
                    modality: parse_index_modality(&modality)?,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: parse_index_state(&state)?,
                    last_error,
                    source_hash,
                    backend_cursor,
                    model_name,
                    model_dimension: model_dimension.map(checked_model_dimension).transpose()?,
                })
            },
        )
        .collect()
}

pub(super) fn queue_index_refreshes(
    connection: &mut Connection,
    request: IndexRefreshQueueRequest,
) -> Result<IndexRefreshDiagnostics, StorageError> {
    task_queue::queue_index_refreshes(connection, request)
}

pub(super) fn claim_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshClaimRequest,
) -> Result<Option<IndexRefreshTask>, StorageError> {
    task_queue::claim_index_refresh_task(connection, request)
}

pub(super) fn complete_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshCompletion,
) -> Result<IndexRefreshTask, StorageError> {
    task_queue::complete_index_refresh_task(connection, request)
}

pub(super) fn fail_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshFailure,
) -> Result<IndexRefreshTask, StorageError> {
    task_queue::fail_index_refresh_task(connection, request)
}

pub(super) fn diagnostics(
    connection: &Connection,
    now_ms: u64,
) -> Result<IndexRefreshDiagnostics, StorageError> {
    let queue_depth = unfinished_task_count(connection)?;
    let running_count = task_state_count(connection, IndexRefreshTaskState::Running)?;
    let retrying_count = task_state_count(connection, IndexRefreshTaskState::Retrying)?;
    let dead_letter_count = task_state_count(connection, IndexRefreshTaskState::DeadLetter)?;
    let oldest_unfinished_age_ms = oldest_unfinished_created_at(connection)?
        .map(|created_at| now_ms.saturating_sub(created_at));
    let graph_version = current_graph_version(connection)?;
    let statuses = index_statuses(connection)?;
    let mut max_index_lag_versions = 0;
    let index_lag_by_kind = statuses
        .iter()
        .map(|status| {
            let lag = graph_version
                .get()
                .saturating_sub(status.indexed_graph_version.get());
            max_index_lag_versions = max_index_lag_versions.max(lag);

            IndexLag {
                kind: status.kind,
                lag_versions: lag,
            }
        })
        .collect::<Vec<_>>();
    let stale_index_count = statuses
        .iter()
        .filter(|status| status.is_stale_for(graph_version))
        .count();

    Ok(IndexRefreshDiagnostics {
        queue_depth,
        running_count,
        retrying_count,
        dead_letter_count,
        oldest_unfinished_age_ms,
        index_lag_by_kind,
        max_index_lag_versions,
        stale_index_count,
    })
}

pub(super) fn parse_json_array(value: String) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(&value).map_err(|error| {
        StorageError::InvalidInput(format!("invalid mutation log JSON array: {error}"))
    })
}

pub(super) fn source_hash(source_scope: &str, source_path: Option<&str>, content: &str) -> String {
    let mut input = Vec::new();
    append_hash_part(&mut input, source_scope);
    append_hash_part(&mut input, source_path.unwrap_or(""));
    append_hash_part(&mut input, content);

    format!("{:016x}", stable_hash64(&input))
}

pub(super) fn json_array(values: impl IntoIterator<Item = String>) -> Result<String, StorageError> {
    let unique = values.into_iter().collect::<BTreeSet<_>>();

    serde_json::to_string(&unique.into_iter().collect::<Vec<_>>())
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn ensure_cursor(
    connection: &Connection,
    kind: IndexKind,
    scope: &str,
    modality: IndexModality,
    state: IndexState,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT OR IGNORE INTO index_scope_manifest (source_scope) VALUES (?1)",
        params![scope],
    )?;
    connection.execute(
        "
        INSERT OR IGNORE INTO index_cursors (
            kind, source_scope, modality, index_version,
            indexed_graph_version, state, last_error
        )
        VALUES (?1, ?2, ?3, 0, 0, ?4, NULL)
        ",
        params![kind.as_str(), scope, modality.as_str(), state.as_str()],
    )?;

    Ok(())
}

fn mark_cursor_complete(
    connection: &Connection,
    kind: IndexKind,
    scope: &str,
    modality: IndexModality,
    graph_version: GraphVersion,
    error: Option<&str>,
    metadata: &CursorBackendMetadata,
) -> Result<(), StorageError> {
    ensure_cursor(connection, kind, scope, modality, IndexState::Stale)?;
    connection.execute(
        "
        UPDATE index_cursors
        SET index_version = index_version + 1,
            indexed_graph_version = ?4,
            state = 'fresh',
            last_error = ?5,
            source_hash = ?6,
            backend_cursor = ?7,
            model_name = ?8,
            model_dimension = ?9
        WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
        ",
        params![
            kind.as_str(),
            scope,
            modality.as_str(),
            graph_version.get(),
            error,
            &metadata.source_hash,
            &metadata.backend_cursor,
            metadata.model_name.as_deref(),
            metadata.model_dimension
        ],
    )?;

    Ok(())
}

fn mark_cursor_stale_at(
    connection: &Connection,
    kind: IndexKind,
    scope: &str,
    modality: IndexModality,
    graph_version: GraphVersion,
    error: Option<&str>,
    metadata: &CursorBackendMetadata,
) -> Result<(), StorageError> {
    ensure_cursor(connection, kind, scope, modality, IndexState::Stale)?;
    connection.execute(
        "
        UPDATE index_cursors
        SET index_version = index_version + 1,
            indexed_graph_version = ?4,
            state = 'stale',
            last_error = ?5,
            source_hash = ?6,
            backend_cursor = ?7,
            model_name = ?8,
            model_dimension = ?9
        WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
        ",
        params![
            kind.as_str(),
            scope,
            modality.as_str(),
            graph_version.get(),
            error,
            &metadata.source_hash,
            &metadata.backend_cursor,
            metadata.model_name.as_deref(),
            metadata.model_dimension
        ],
    )?;

    Ok(())
}

fn recompute_aggregate_status(
    connection: &Connection,
    kind: IndexKind,
    fresh_graph_version_floor: GraphVersion,
) -> Result<(), StorageError> {
    let graph_version = current_graph_version(connection)?.max(fresh_graph_version_floor);
    let failed_error = first_failed_cursor_error(connection, kind)?;
    let has_unfinished = unfinished_task_for_kind_count(connection, kind)? > 0;
    let has_stale_cursor = stale_cursor_count(connection, kind)? > 0;
    let has_missing_cursor = !missing_cursor_scopes(connection, kind)?.is_empty();
    let current = read_index_status(connection, kind)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "index status row for '{}' is missing",
            kind.as_str()
        ))
    })?;

    let (state, indexed_graph_version, last_error) = if let Some(error) = failed_error {
        (
            IndexState::Failed,
            current.indexed_graph_version,
            Some(error),
        )
    } else if has_unfinished || has_stale_cursor || has_missing_cursor {
        (IndexState::Stale, current.indexed_graph_version, None)
    } else {
        (IndexState::Fresh, graph_version, None)
    };
    let updated = connection.execute(
        "
        UPDATE index_status
        SET index_version = index_version + 1,
            indexed_graph_version = ?2,
            state = ?3,
            last_error = ?4
        WHERE kind = ?1
        ",
        params![
            kind.as_str(),
            indexed_graph_version.get(),
            state.as_str(),
            last_error
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::InvalidInput(format!(
            "index status row for '{}' was not updated",
            kind.as_str()
        )));
    }

    Ok(())
}

fn first_failed_cursor_error(
    connection: &Connection,
    kind: IndexKind,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT last_error
            FROM index_cursors
            WHERE kind = ?1 AND state = 'failed'
            ORDER BY source_scope ASC, modality ASC
            LIMIT 1
            ",
            params![kind.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(StorageError::from)
}

fn stale_cursor_count(connection: &Connection, kind: IndexKind) -> Result<usize, StorageError> {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM index_cursors
            WHERE kind = ?1 AND state != 'fresh'
            ",
            params![kind.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn missing_cursor_scopes(
    connection: &Connection,
    kind: IndexKind,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT manifest.source_scope
        FROM index_scope_manifest manifest
        WHERE NOT EXISTS (
            SELECT 1
            FROM index_cursors cursor
            WHERE cursor.kind = ?1
              AND cursor.source_scope = manifest.source_scope
              AND cursor.modality = ?2
        )
        ORDER BY manifest.source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![kind.as_str(), TEXT_MODALITY.as_str()], |row| {
        row.get::<_, String>(0)
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn apply_cursor_integrity(
    connection: &Connection,
    status: &mut IndexStatus,
) -> Result<(), StorageError> {
    let missing = missing_cursor_scopes(connection, status.kind)?.len();
    if missing > 0 && status.state == IndexState::Fresh {
        status.state = IndexState::Stale;
        status.last_error = Some(format!("{missing} scoped index cursor(s) missing"));
    }

    Ok(())
}

fn unfinished_task_count(connection: &Connection) -> Result<usize, StorageError> {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM index_refresh_tasks
            WHERE state IN ('queued', 'running', 'retrying', 'failed')
            ",
            [],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

fn unfinished_task_for_kind_count(
    connection: &Connection,
    kind: IndexKind,
) -> Result<usize, StorageError> {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM index_refresh_tasks
            WHERE kind = ?1 AND state IN ('queued', 'running', 'retrying', 'failed')
            ",
            params![kind.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

fn task_state_count(
    connection: &Connection,
    state: IndexRefreshTaskState,
) -> Result<usize, StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM index_refresh_tasks WHERE state = ?1",
            params![state.as_str()],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)
}

fn oldest_unfinished_created_at(connection: &Connection) -> Result<Option<u64>, StorageError> {
    connection
        .query_row(
            "
            SELECT MIN(created_at_ms)
            FROM index_refresh_tasks
            WHERE state IN ('queued', 'running', 'retrying', 'failed')
            ",
            [],
            |row| row.get::<_, Option<u64>>(0),
        )
        .map_err(StorageError::from)
}

fn read_index_status(
    connection: &Connection,
    kind: IndexKind,
) -> Result<Option<IndexStatus>, StorageError> {
    let raw_status = connection
        .query_row(
            "
            SELECT index_version, indexed_graph_version, state, last_error
            FROM index_status
            WHERE kind = ?1
            ",
            params![kind.as_str()],
            |row| {
                let state: String = row.get(2)?;
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    state,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(StorageError::from)?;

    raw_status
        .map(
            |(index_version, indexed_graph_version, state, last_error)| {
                Ok(IndexStatus {
                    kind,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: parse_index_state(&state)?,
                    last_error,
                })
            },
        )
        .transpose()
}

fn current_graph_version(connection: &Connection) -> Result<GraphVersion, StorageError> {
    let value = connection.query_row(
        "SELECT graph_version FROM graph_state WHERE id = 1",
        [],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(GraphVersion::new(value))
}

fn parse_index_kind(value: &str) -> Result<IndexKind, StorageError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        _ => Err(invalid_index_metadata(format!(
            "unknown index kind '{value}'"
        ))),
    }
}

fn parse_index_modality(value: &str) -> Result<IndexModality, StorageError> {
    match value {
        "text" => Ok(IndexModality::Text),
        "image" => Ok(IndexModality::Image),
        "layout" => Ok(IndexModality::Layout),
        "table" => Ok(IndexModality::Table),
        _ => Err(invalid_index_metadata(format!(
            "unknown index modality '{value}'"
        ))),
    }
}

fn parse_index_state(value: &str) -> Result<IndexState, StorageError> {
    match value {
        "fresh" => Ok(IndexState::Fresh),
        "stale" => Ok(IndexState::Stale),
        "failed" => Ok(IndexState::Failed),
        "paused" => Ok(IndexState::Paused),
        _ => Err(invalid_index_metadata(format!(
            "unknown index state '{value}'"
        ))),
    }
}

fn parse_task_state(value: &str) -> Result<IndexRefreshTaskState, StorageError> {
    match value {
        "queued" => Ok(IndexRefreshTaskState::Queued),
        "running" => Ok(IndexRefreshTaskState::Running),
        "succeeded" => Ok(IndexRefreshTaskState::Succeeded),
        "retrying" => Ok(IndexRefreshTaskState::Retrying),
        "failed" => Ok(IndexRefreshTaskState::Failed),
        "dead_letter" => Ok(IndexRefreshTaskState::DeadLetter),
        _ => Err(invalid_index_metadata(format!(
            "unknown index refresh task state '{value}'"
        ))),
    }
}

fn validate_required_index_statuses(statuses: &[IndexStatus]) -> Result<(), StorageError> {
    for kind in IndexKind::ALL {
        if !statuses.iter().any(|status| status.kind == kind) {
            return Err(invalid_index_metadata(format!(
                "required index status row for '{}' is missing",
                kind.as_str()
            )));
        }
    }

    Ok(())
}

fn invalid_index_metadata(message: String) -> StorageError {
    StorageError::InvalidInput(format!("{message} in storage metadata"))
}

fn invalid_to_sqlite(error: StorageError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
