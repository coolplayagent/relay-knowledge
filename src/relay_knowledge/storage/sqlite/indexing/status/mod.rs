use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality, IndexState, IndexStatus},
    storage::{IndexCursor, StorageError},
};

use super::{
    DEFAULT_SCOPE, TEXT_MODALITY,
    cursor_metadata::{
        CursorBackendMetadata, CursorBackendMetadataRequest, checked_model_dimension,
        cursor_backend_metadata, cursor_indexed_graph_version,
    },
    parse_index_kind, parse_index_modality, parse_index_state, unfinished_task_for_kind_count,
    validate_required_index_statuses,
};

pub(crate) fn mark_mutation_cursors_stale(
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

pub(crate) fn index_statuses(connection: &Connection) -> Result<Vec<IndexStatus>, StorageError> {
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

pub(crate) fn mark_refresh_complete(
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

pub(crate) fn index_cursors(connection: &mut Connection) -> Result<Vec<IndexCursor>, StorageError> {
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
pub(super) fn ensure_cursor(
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

pub(super) fn mark_cursor_complete(
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

pub(super) fn mark_cursor_stale_at(
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

pub(super) fn recompute_aggregate_status(
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

pub(crate) fn missing_cursor_scopes(
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

pub(super) fn read_index_status(
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

pub(super) fn current_graph_version(connection: &Connection) -> Result<GraphVersion, StorageError> {
    let value = connection.query_row(
        "SELECT graph_version FROM graph_state WHERE id = 1",
        [],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(GraphVersion::new(value))
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod status_tests;
