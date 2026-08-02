use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality, IndexState},
    storage::{IndexCursor, IndexRefreshQueueRequest, StorageError},
};

pub(super) struct PlannedTask {
    pub(super) kind: IndexKind,
    pub(super) source_scope: String,
    pub(super) modality: IndexModality,
    pub(super) target_graph_version: GraphVersion,
    pub(super) cursor_before: GraphVersion,
}

pub(super) fn planned_tasks(
    connection: &Connection,
    request: &IndexRefreshQueueRequest,
) -> Result<Vec<PlannedTask>, StorageError> {
    let mut planned = Vec::new();
    for kind in &request.kinds {
        let missing_scopes = super::super::missing_cursor_scopes(connection, *kind)?;
        let missing_scope_set = missing_scopes.iter().cloned().collect::<BTreeSet<_>>();
        for scope in missing_scopes {
            super::super::ensure_cursor(
                connection,
                *kind,
                &scope,
                super::super::TEXT_MODALITY,
                IndexState::Stale,
            )?;
            planned.push(PlannedTask {
                kind: *kind,
                source_scope: scope,
                modality: super::super::TEXT_MODALITY,
                target_graph_version: request.target_graph_version,
                cursor_before: GraphVersion::ZERO,
            });
        }
        let cursors = stale_cursors_for_kind(connection, *kind)?;
        let stale_cursors = cursors
            .into_iter()
            .filter(|cursor| !missing_scope_set.contains(&cursor.source_scope))
            .collect::<Vec<_>>();
        if stale_cursors.is_empty() && missing_scope_set.is_empty() {
            if let Some(cursor_before) =
                fallback_cursor_before(connection, *kind, request.target_graph_version)?
            {
                super::super::ensure_cursor(
                    connection,
                    *kind,
                    super::super::DEFAULT_SCOPE,
                    super::super::TEXT_MODALITY,
                    IndexState::Stale,
                )?;
                planned.push(PlannedTask {
                    kind: *kind,
                    source_scope: super::super::DEFAULT_SCOPE.to_owned(),
                    modality: super::super::TEXT_MODALITY,
                    target_graph_version: request.target_graph_version,
                    cursor_before,
                });
            }
        } else {
            planned.extend(stale_cursors.into_iter().map(|cursor| PlannedTask {
                kind: cursor.kind,
                source_scope: cursor.source_scope,
                modality: cursor.modality,
                target_graph_version: request.target_graph_version,
                cursor_before: cursor.indexed_graph_version,
            }));
        }
    }

    Ok(planned)
}

fn stale_cursors_for_kind(
    connection: &Connection,
    kind: IndexKind,
) -> Result<Vec<IndexCursor>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, source_scope, modality, index_version,
               indexed_graph_version, state, last_error,
               source_hash, backend_cursor, model_name, model_dimension
        FROM index_cursors
        WHERE kind = ?1
          AND state != 'fresh'
        ORDER BY source_scope ASC, modality ASC
        ",
    )?;
    let rows = statement.query_map(params![kind.as_str()], |row| {
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
                    kind: super::super::parse_index_kind(&kind)?,
                    source_scope,
                    modality: super::super::parse_index_modality(&modality)?,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: super::super::parse_index_state(&state)?,
                    last_error,
                    source_hash,
                    backend_cursor,
                    model_name,
                    model_dimension: model_dimension
                        .map(super::super::cursor_metadata::checked_model_dimension)
                        .transpose()?,
                })
            },
        )
        .collect()
}

fn fallback_cursor_before(
    connection: &Connection,
    kind: IndexKind,
    graph_version: GraphVersion,
) -> Result<Option<GraphVersion>, StorageError> {
    let Some(status) = super::super::read_index_status(connection, kind)? else {
        return Ok(Some(GraphVersion::ZERO));
    };
    if status.is_stale_for(graph_version) {
        Ok(Some(status.indexed_graph_version))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod planning_tests;
