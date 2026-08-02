//! Index-family, scoped-cursor, queue, lag, and stale diagnostics.

use rusqlite::{Connection, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexState, IndexStatus},
    storage::{
        IndexLag, IndexRefreshDiagnostics, IndexRefreshTaskState, IndexStalenessReason,
        StorageError,
    },
};

use super::{
    current_graph_version, index_statuses, parse_index_kind, parse_index_modality,
    parse_index_state,
};

pub(crate) fn diagnostics(
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
    let stale_reasons = stale_reasons(connection, graph_version, &statuses)?;

    Ok(IndexRefreshDiagnostics {
        queue_depth,
        running_count,
        retrying_count,
        dead_letter_count,
        oldest_unfinished_age_ms,
        index_lag_by_kind,
        max_index_lag_versions,
        stale_index_count,
        stale_reasons,
    })
}

fn stale_reasons(
    connection: &Connection,
    graph_version: GraphVersion,
    statuses: &[IndexStatus],
) -> Result<Vec<IndexStalenessReason>, StorageError> {
    let mut reasons = statuses
        .iter()
        .filter(|status| status.is_stale_for(graph_version) || status.last_error.is_some())
        .map(|status| {
            let lag = graph_version
                .get()
                .saturating_sub(status.indexed_graph_version.get());

            IndexStalenessReason {
                kind: status.kind,
                source_scope: None,
                modality: None,
                reason: index_status_reason(status.state, lag, status.last_error.is_some())
                    .to_owned(),
                lag_versions: lag,
                last_error: status.last_error.clone(),
            }
        })
        .collect::<Vec<_>>();
    reasons.extend(stale_cursor_reasons(connection, graph_version)?);

    Ok(reasons)
}

fn stale_cursor_reasons(
    connection: &Connection,
    graph_version: GraphVersion,
) -> Result<Vec<IndexStalenessReason>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, source_scope, modality, indexed_graph_version, state, last_error
        FROM index_cursors
        WHERE state != 'fresh'
           OR indexed_graph_version < ?1
           OR last_error IS NOT NULL
        ORDER BY kind ASC, source_scope ASC, modality ASC
        ",
    )?;
    let rows = statement.query_map(params![graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(
            |(kind, source_scope, modality, indexed_graph_version, state, last_error)| {
                let state = parse_index_state(&state)?;
                let lag = graph_version.get().saturating_sub(indexed_graph_version);

                Ok(IndexStalenessReason {
                    kind: parse_index_kind(&kind)?,
                    source_scope: Some(source_scope),
                    modality: Some(parse_index_modality(&modality)?),
                    reason: index_cursor_reason(state, lag, last_error.is_some()).to_owned(),
                    lag_versions: lag,
                    last_error,
                })
            },
        )
        .collect()
}

fn index_status_reason(state: IndexState, lag: u64, has_error: bool) -> &'static str {
    if state == IndexState::Failed {
        "index family failed"
    } else if lag > 0 {
        "index family lags graph version"
    } else if state != IndexState::Fresh {
        "index family is not fresh"
    } else if has_error {
        "index family reports last error"
    } else {
        "index family is fresh"
    }
}

fn index_cursor_reason(state: IndexState, lag: u64, has_error: bool) -> &'static str {
    if state == IndexState::Failed {
        "scoped cursor failed"
    } else if lag > 0 {
        "scoped cursor lags graph version"
    } else if state != IndexState::Fresh {
        "scoped cursor is not fresh"
    } else if has_error {
        "scoped cursor reports last error"
    } else {
        "scoped cursor is fresh"
    }
}
pub(super) fn unfinished_task_count(connection: &Connection) -> Result<usize, StorageError> {
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

pub(super) fn unfinished_task_for_kind_count(
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
