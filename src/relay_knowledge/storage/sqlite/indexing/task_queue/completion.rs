use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{IndexRefreshCompletion, IndexRefreshTask, IndexRefreshTaskState, StorageError},
};

use super::record::{inactive_lease_error, input_fingerprint, require_task};

pub(crate) fn complete_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshCompletion,
) -> Result<IndexRefreshTask, StorageError> {
    let transaction = connection.transaction()?;
    let task = require_task(&transaction, &request.task_id)?;
    let superseded = task.target_graph_version > request.indexed_graph_version
        || has_matching_mutation_after(&transaction, &task, request.indexed_graph_version)?;
    let metadata = super::super::cursor_metadata::cursor_backend_metadata(
        &transaction,
        super::super::cursor_metadata::CursorBackendMetadataRequest {
            kind: task.kind,
            scope: &task.source_scope,
            modality: task.modality,
            cursor_before: task.cursor_before,
            graph_version: request.indexed_graph_version,
            model_name: request.model_name.as_deref(),
            model_dimension: request.model_dimension,
        },
    )?;
    let next_target = if superseded {
        super::super::current_graph_version(&transaction)?.max(task.target_graph_version)
    } else {
        task.target_graph_version
    };
    let next_state = if superseded {
        IndexRefreshTaskState::Queued
    } else {
        IndexRefreshTaskState::Succeeded
    };
    let next_cursor_before = if superseded {
        request.indexed_graph_version
    } else {
        task.cursor_before
    };
    let next_cursor_after = if superseded {
        None
    } else {
        Some(request.indexed_graph_version.get())
    };
    let next_attempt_count = if superseded { 0 } else { task.attempt_count };
    let next_fingerprint =
        input_fingerprint(task.kind, &task.source_scope, task.modality, next_target);
    let updated = transaction.execute(
        "
        UPDATE index_refresh_tasks
        SET state = ?5,
            target_graph_version = ?6,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?7,
            input_fingerprint = ?8,
            cursor_before = ?9,
            cursor_after = ?10,
            attempt_count = ?11,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?12
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?4
        ",
        params![
            &request.task_id,
            &request.lease_owner,
            request.attempt_count,
            request.now_ms,
            next_state.as_str(),
            next_target.get(),
            request.now_ms,
            next_fingerprint,
            next_cursor_before.get(),
            next_cursor_after,
            next_attempt_count,
            request.now_ms
        ],
    )?;
    if updated != 1 {
        return Err(inactive_lease_error(&request.task_id));
    }
    if superseded {
        super::super::mark_cursor_stale_at(
            &transaction,
            task.kind,
            &task.source_scope,
            task.modality,
            request.indexed_graph_version,
            None,
            &metadata,
        )?;
    } else {
        super::super::mark_cursor_complete(
            &transaction,
            task.kind,
            &task.source_scope,
            task.modality,
            request.indexed_graph_version,
            None,
            &metadata,
        )?;
    }
    super::super::recompute_aggregate_status(&transaction, task.kind, GraphVersion::ZERO)?;
    transaction.commit()?;

    require_task(connection, &task.task_id)
}

fn has_matching_mutation_after(
    connection: &Connection,
    task: &IndexRefreshTask,
    graph_version: GraphVersion,
) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT affected_scopes_json
        FROM graph_mutations
        WHERE graph_version > ?1
        ORDER BY graph_version ASC
        ",
    )?;
    let mut rows = statement.query(params![graph_version.get()])?;

    while let Some(row) = rows.next()? {
        if task.source_scope == super::super::DEFAULT_SCOPE {
            return Ok(true);
        }
        let scopes_json = row.get::<_, String>(0)?;
        let scopes = super::super::parse_json_array(scopes_json)?;
        if scopes.iter().any(|scope| scope == &task.source_scope) {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod completion_tests;
