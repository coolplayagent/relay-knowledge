use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality, IndexState},
    storage::{
        IndexCursor, IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshDiagnostics,
        IndexRefreshFailure, IndexRefreshQueueRequest, IndexRefreshTask, IndexRefreshTaskState,
        StorageError,
    },
};

struct PlannedTask {
    kind: IndexKind,
    source_scope: String,
    modality: IndexModality,
    target_graph_version: GraphVersion,
    cursor_before: GraphVersion,
}

pub(super) fn queue_index_refreshes(
    connection: &mut Connection,
    request: IndexRefreshQueueRequest,
) -> Result<IndexRefreshDiagnostics, StorageError> {
    if request.max_queue_depth == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh queue capacity must be greater than zero".to_owned(),
        ));
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let tasks = planned_tasks(&transaction, &request)?;
    let new_task_count = tasks
        .iter()
        .map(|task| task_id(task.kind, &task.source_scope, task.modality))
        .map(|id| match read_task(&transaction, &id) {
            Ok(task) => task.map_or(Ok(true), |task| {
                Ok(task_needs_enqueue(&task, request.reset_dead_letter_tasks))
            }),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|value| *value)
        .count();
    let current_depth = super::unfinished_task_count(&transaction)?;
    if current_depth.saturating_add(new_task_count) > request.max_queue_depth {
        return Err(StorageError::InvalidInput(format!(
            "index refresh queue capacity exceeded: depth={} new={} capacity={}",
            current_depth, new_task_count, request.max_queue_depth
        )));
    }

    for task in tasks {
        upsert_task(
            &transaction,
            task,
            request.now_ms,
            request.reset_dead_letter_tasks,
        )?;
    }

    let diagnostics = super::diagnostics(&transaction, request.now_ms)?;
    transaction.commit()?;

    Ok(diagnostics)
}

pub(super) fn claim_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshClaimRequest,
) -> Result<Option<IndexRefreshTask>, StorageError> {
    let lease_owner = request.lease_owner.trim();
    if lease_owner.is_empty() {
        return Err(StorageError::InvalidInput(
            "index refresh lease owner must not be empty".to_owned(),
        ));
    }
    if request.lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh lease duration must be greater than zero".to_owned(),
        ));
    }
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh max attempts must be greater than zero".to_owned(),
        ));
    }

    recover_expired_leases(connection, request.now_ms, request.max_attempts)?;
    loop {
        let task_id = connection
            .query_row(
                "
                SELECT task_id
                FROM index_refresh_tasks
                WHERE state = 'queued'
                   OR (state = 'retrying' AND next_retry_at_ms <= ?1)
                ORDER BY created_at_ms ASC, target_graph_version ASC, task_id ASC
                LIMIT 1
                ",
                params![request.now_ms],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let Some(task_id) = task_id else {
            return Ok(None);
        };
        let updated = connection.execute(
            "
            UPDATE index_refresh_tasks
            SET state = 'running',
                lease_owner = ?2,
                lease_expires_at_ms = ?3,
                attempt_count = attempt_count + 1,
                updated_at_ms = ?4
            WHERE task_id = ?1
              AND (
                  state = 'queued'
                  OR (state = 'retrying' AND next_retry_at_ms <= ?4)
              )
            ",
            params![
                task_id,
                lease_owner,
                request.now_ms.saturating_add(request.lease_duration_ms),
                request.now_ms
            ],
        )?;

        if updated == 1 {
            return read_task(connection, &task_id)?.map(Some).ok_or_else(|| {
                StorageError::InvalidInput("claimed index refresh task is missing".to_owned())
            });
        }
    }
}

pub(super) fn complete_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshCompletion,
) -> Result<IndexRefreshTask, StorageError> {
    let transaction = connection.transaction()?;
    let task = require_task(&transaction, &request.task_id)?;
    let superseded = task.target_graph_version > request.indexed_graph_version
        || has_matching_mutation_after(&transaction, &task, request.indexed_graph_version)?;
    let next_target = if superseded {
        super::current_graph_version(&transaction)?.max(task.target_graph_version)
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
        super::mark_cursor_stale_at(
            &transaction,
            task.kind,
            &task.source_scope,
            task.modality,
            request.indexed_graph_version,
            None,
        )?;
    } else {
        super::mark_cursor_complete(
            &transaction,
            task.kind,
            &task.source_scope,
            task.modality,
            request.indexed_graph_version,
            None,
        )?;
    }
    super::recompute_aggregate_status(&transaction, task.kind, GraphVersion::ZERO)?;
    transaction.commit()?;

    require_task(connection, &task.task_id)
}

pub(super) fn fail_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshFailure,
) -> Result<IndexRefreshTask, StorageError> {
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh max attempts must be greater than zero".to_owned(),
        ));
    }
    let transaction = connection.transaction()?;
    let task = require_task(&transaction, &request.task_id)?;
    let next_state = if task.attempt_count >= request.max_attempts {
        IndexRefreshTaskState::DeadLetter
    } else {
        IndexRefreshTaskState::Retrying
    };
    let next_retry = request.now_ms.saturating_add(request.retry_backoff_ms);
    let updated = transaction.execute(
        "
        UPDATE index_refresh_tasks
        SET state = ?5,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?6,
            last_error_kind = ?7,
            last_error_message = ?8,
            updated_at_ms = ?9
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
            next_retry,
            &request.error_kind,
            &request.error_message,
            request.now_ms
        ],
    )?;
    if updated != 1 {
        return Err(inactive_lease_error(&request.task_id));
    }
    transaction.execute(
        "
        UPDATE index_cursors
        SET state = 'failed', last_error = ?4
        WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
        ",
        params![
            task.kind.as_str(),
            &task.source_scope,
            task.modality.as_str(),
            &request.error_message
        ],
    )?;
    super::recompute_aggregate_status(&transaction, task.kind, GraphVersion::ZERO)?;
    transaction.commit()?;

    require_task(connection, &task.task_id)
}

fn planned_tasks(
    connection: &Connection,
    request: &IndexRefreshQueueRequest,
) -> Result<Vec<PlannedTask>, StorageError> {
    let mut planned = Vec::new();
    for kind in &request.kinds {
        let cursors = stale_cursors_for_kind(connection, *kind)?;
        if cursors.is_empty() {
            if let Some(cursor_before) =
                fallback_cursor_before(connection, *kind, request.target_graph_version)?
            {
                super::ensure_cursor(
                    connection,
                    *kind,
                    super::DEFAULT_SCOPE,
                    super::TEXT_MODALITY,
                    IndexState::Stale,
                )?;
                planned.push(PlannedTask {
                    kind: *kind,
                    source_scope: super::DEFAULT_SCOPE.to_owned(),
                    modality: super::TEXT_MODALITY,
                    target_graph_version: request.target_graph_version,
                    cursor_before,
                });
            }
        } else {
            planned.extend(cursors.into_iter().map(|cursor| PlannedTask {
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
               indexed_graph_version, state, last_error
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
            )| {
                Ok(IndexCursor {
                    kind: super::parse_index_kind(&kind)?,
                    source_scope,
                    modality: super::parse_index_modality(&modality)?,
                    index_version,
                    indexed_graph_version: GraphVersion::new(indexed_graph_version),
                    state: super::parse_index_state(&state)?,
                    last_error,
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
    let Some(status) = super::read_index_status(connection, kind)? else {
        return Ok(Some(GraphVersion::ZERO));
    };
    if status.is_stale_for(graph_version) {
        Ok(Some(status.indexed_graph_version))
    } else {
        Ok(None)
    }
}

fn upsert_task(
    connection: &Connection,
    task: PlannedTask,
    now_ms: u64,
    reset_dead_letter_tasks: bool,
) -> Result<(), StorageError> {
    let task_id = task_id(task.kind, &task.source_scope, task.modality);
    let input_fingerprint = input_fingerprint(
        task.kind,
        &task.source_scope,
        task.modality,
        task.target_graph_version,
    );
    let existing = read_task(connection, &task_id)?;
    match existing {
        None => insert_task(connection, task, task_id, input_fingerprint, now_ms),
        Some(existing) if existing.state == IndexRefreshTaskState::Succeeded => {
            if existing
                .cursor_after
                .is_some_and(|version| version >= task.target_graph_version)
            {
                Ok(())
            } else {
                reset_task(connection, task, task_id, input_fingerprint, now_ms)
            }
        }
        Some(existing) if existing.state == IndexRefreshTaskState::DeadLetter => {
            if !reset_dead_letter_tasks {
                return Ok(());
            }
            reset_task(connection, task, task_id, input_fingerprint, now_ms)
        }
        Some(existing) if existing.state == IndexRefreshTaskState::Running => Ok(()),
        Some(existing) => {
            let target = existing.target_graph_version.max(task.target_graph_version);
            connection.execute(
                "
                UPDATE index_refresh_tasks
                SET target_graph_version = ?2,
                    input_fingerprint = ?3,
                    cursor_before = MIN(cursor_before, ?4),
                    updated_at_ms = ?5
                WHERE task_id = ?1
                ",
                params![
                    task_id,
                    target.get(),
                    input_fingerprint,
                    task.cursor_before.get(),
                    now_ms
                ],
            )?;
            Ok(())
        }
    }
}

fn insert_task(
    connection: &Connection,
    task: PlannedTask,
    task_id: String,
    input_fingerprint: String,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT INTO index_refresh_tasks (
            task_id, kind, source_scope, modality, target_graph_version, state,
            lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
            input_fingerprint, cursor_before, cursor_after, last_error_kind,
            last_error_message, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', NULL, NULL, 0, ?6,
                ?7, ?8, NULL, NULL, NULL, ?9, ?9)
        ",
        params![
            task_id,
            task.kind.as_str(),
            task.source_scope,
            task.modality.as_str(),
            task.target_graph_version.get(),
            now_ms,
            input_fingerprint,
            task.cursor_before.get(),
            now_ms
        ],
    )?;

    Ok(())
}

fn reset_task(
    connection: &Connection,
    task: PlannedTask,
    task_id: String,
    input_fingerprint: String,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET target_graph_version = ?2,
            state = 'queued',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            attempt_count = 0,
            next_retry_at_ms = ?3,
            input_fingerprint = ?4,
            cursor_before = ?5,
            cursor_after = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?6
        WHERE task_id = ?1
        ",
        params![
            task_id,
            task.target_graph_version.get(),
            now_ms,
            input_fingerprint,
            task.cursor_before.get(),
            now_ms
        ],
    )?;

    Ok(())
}

fn task_needs_enqueue(task: &IndexRefreshTask, reset_dead_letter_tasks: bool) -> bool {
    match task.state {
        IndexRefreshTaskState::Queued
        | IndexRefreshTaskState::Running
        | IndexRefreshTaskState::Retrying
        | IndexRefreshTaskState::Failed => false,
        IndexRefreshTaskState::DeadLetter => reset_dead_letter_tasks,
        IndexRefreshTaskState::Succeeded => true,
    }
}

fn task_id(kind: IndexKind, source_scope: &str, modality: IndexModality) -> String {
    let mut input = Vec::new();
    super::append_hash_part(&mut input, kind.as_str());
    super::append_hash_part(&mut input, source_scope);
    super::append_hash_part(&mut input, modality.as_str());

    format!("index-refresh:{:016x}", super::stable_hash64(&input))
}

fn input_fingerprint(
    kind: IndexKind,
    source_scope: &str,
    modality: IndexModality,
    target_graph_version: GraphVersion,
) -> String {
    format!(
        "{}:{}:{}:{}",
        kind.as_str(),
        super::stable_hash64(source_scope.as_bytes()),
        modality.as_str(),
        target_graph_version.get()
    )
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
        if task.source_scope == super::DEFAULT_SCOPE {
            return Ok(true);
        }
        let scopes_json = row.get::<_, String>(0)?;
        let scopes = super::parse_json_array(scopes_json)?;
        if scopes.iter().any(|scope| scope == &task.source_scope) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn recover_expired_leases(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    let dead_letter_kinds = expired_dead_letter_kinds(connection, now_ms, max_attempts)?;
    connection.execute(
        "
        UPDATE index_cursors
        SET state = 'failed',
            last_error = 'index refresh task lease expired'
        WHERE EXISTS (
            SELECT 1
            FROM index_refresh_tasks task
            WHERE task.kind = index_cursors.kind
              AND task.source_scope = index_cursors.source_scope
              AND task.modality = index_cursors.modality
              AND task.state = 'running'
              AND task.lease_expires_at_ms IS NOT NULL
              AND task.lease_expires_at_ms <= ?1
              AND task.attempt_count >= ?2
        )
        ",
        params![now_ms, max_attempts],
    )?;
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET state = CASE
                WHEN attempt_count >= ?2 THEN 'dead_letter'
                ELSE 'retrying'
            END,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?1,
            last_error_kind = 'lease_expired',
            last_error_message = 'index refresh task lease expired',
            updated_at_ms = ?1
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
        ",
        params![now_ms, max_attempts],
    )?;
    for kind in dead_letter_kinds {
        super::recompute_aggregate_status(connection, kind, GraphVersion::ZERO)?;
    }

    Ok(())
}

fn expired_dead_letter_kinds(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<Vec<IndexKind>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT kind
        FROM index_refresh_tasks
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
          AND attempt_count >= ?2
        ",
    )?;
    let rows = statement.query_map(params![now_ms, max_attempts], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|kind| super::parse_index_kind(&kind))
        .collect()
}

fn inactive_lease_error(task_id: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "index refresh task '{task_id}' is not held by an active lease"
    ))
}

fn read_task(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<IndexRefreshTask>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, modality, target_graph_version,
                   state, lease_owner, lease_expires_at_ms, attempt_count,
                   next_retry_at_ms, input_fingerprint, cursor_before, cursor_after,
                   last_error_kind, last_error_message, created_at_ms, updated_at_ms
            FROM index_refresh_tasks
            WHERE task_id = ?1
            ",
            params![task_id],
            task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn require_task(connection: &Connection, task_id: &str) -> Result<IndexRefreshTask, StorageError> {
    read_task(connection, task_id)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("index refresh task '{task_id}' is missing"))
    })
}

fn task_from_row(row: &rusqlite::Row<'_>) -> Result<IndexRefreshTask, rusqlite::Error> {
    let kind: String = row.get(1)?;
    let modality: String = row.get(3)?;
    let state: String = row.get(5)?;

    Ok(IndexRefreshTask {
        task_id: row.get(0)?,
        kind: super::parse_index_kind(&kind).map_err(super::invalid_to_sqlite)?,
        source_scope: row.get(2)?,
        modality: super::parse_index_modality(&modality).map_err(super::invalid_to_sqlite)?,
        target_graph_version: GraphVersion::new(row.get(4)?),
        state: super::parse_task_state(&state).map_err(super::invalid_to_sqlite)?,
        lease_owner: row.get(6)?,
        lease_expires_at_ms: row.get(7)?,
        attempt_count: row.get(8)?,
        next_retry_at_ms: row.get(9)?,
        input_fingerprint: row.get(10)?,
        cursor_before: GraphVersion::new(row.get(11)?),
        cursor_after: row.get::<_, Option<u64>>(12)?.map(GraphVersion::new),
        last_error_kind: row.get(13)?,
        last_error_message: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
    })
}
