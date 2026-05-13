use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    domain::{
        AuditEventRecord, AuditStatus, GraphVersion, ProposalConflictRecord,
        ProposalConflictSeverity, ProposalKind, ProposalRecord, ProposalState,
        ServiceOperatorState, ServiceOperatorStatus, WorkerBackendState, WorkerKind, WorkerStatus,
        WorkerTaskRecord, WorkerTaskState,
    },
    storage::{
        AuditQueryRequest, NewAuditEvent, NewProposal, NewProposalConflict, ProposalDecision,
        ProposalListRequest, ServiceOperatorUpdate, StorageError, WorkerTaskClaimRequest,
        WorkerTaskCompletion, WorkerTaskFailure, WorkerTaskSeed,
    },
};

/// Initializes durable operational tables.
pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS worker_tasks (
            task_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            evidence_id TEXT,
            target_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE (kind, input_fingerprint)
        );

        CREATE INDEX IF NOT EXISTS worker_tasks_claimable
            ON worker_tasks(kind, state, next_retry_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS proposals (
            proposal_id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            origin TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            decided_by TEXT,
            decision_reason TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS proposals_state_updated
            ON proposals(state, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS proposal_conflicts (
            conflict_id TEXT PRIMARY KEY,
            proposal_id TEXT NOT NULL,
            existing_fact_kind TEXT NOT NULL,
            existing_fact_id TEXT NOT NULL,
            severity TEXT NOT NULL,
            reason TEXT NOT NULL,
            FOREIGN KEY (proposal_id) REFERENCES proposals(proposal_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS proposal_conflicts_by_proposal
            ON proposal_conflicts(proposal_id);

        CREATE TABLE IF NOT EXISTS audit_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            operation TEXT NOT NULL,
            interface TEXT NOT NULL,
            request_id TEXT NOT NULL,
            trace_id TEXT NOT NULL,
            status TEXT NOT NULL,
            actor TEXT,
            source_scope TEXT,
            graph_version INTEGER NOT NULL,
            detail_json TEXT NOT NULL,
            message TEXT,
            created_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS audit_events_operation_sequence
            ON audit_events(operation, sequence DESC);

        CREATE TABLE IF NOT EXISTS service_operator_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            state TEXT NOT NULL,
            silent_updates_enabled INTEGER NOT NULL,
            allowed_scopes_json TEXT NOT NULL,
            last_run_at_ms INTEGER,
            next_retry_at_ms INTEGER,
            last_error TEXT,
            updated_at_ms INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO service_operator_state (
            id, state, silent_updates_enabled, allowed_scopes_json, updated_at_ms
        ) VALUES (1, 'disabled', 0, '[]', 0);
        ",
    )?;

    Ok(())
}

pub(super) fn queue_worker_tasks(
    connection: &Connection,
    tasks: Vec<WorkerTaskSeed>,
) -> Result<Vec<WorkerTaskRecord>, StorageError> {
    let mut records = Vec::with_capacity(tasks.len());
    for task in tasks {
        let task_id = worker_task_id(task.kind, &task.input_fingerprint);
        connection.execute(
            "
            INSERT OR IGNORE INTO worker_tasks (
                task_id, kind, source_scope, evidence_id, target_graph_version, state,
                attempt_count, next_retry_at_ms, input_fingerprint, payload_json,
                created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?7, ?8, ?9, ?9)
            ",
            params![
                task_id,
                task.kind.as_str(),
                task.source_scope,
                task.evidence_id,
                task.target_graph_version.get(),
                task.now_ms,
                task.input_fingerprint,
                task.payload_json,
                task.now_ms,
            ],
        )?;
        let record = worker_task_by_kind_fingerprint(connection, task.kind, task_id.clone())?
            .ok_or_else(|| StorageError::InvalidInput("worker task was not queued".to_owned()))?;
        records.push(record);
    }

    Ok(records)
}

pub(super) fn worker_statuses(connection: &Connection) -> Result<Vec<WorkerStatus>, StorageError> {
    WorkerKind::ALL
        .into_iter()
        .map(|kind| worker_status(connection, kind))
        .collect()
}

pub(super) fn claim_worker_task(
    connection: &Connection,
    request: WorkerTaskClaimRequest,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    let kind_filter = request.kind.map(|kind| kind.as_str().to_owned());
    let row_id = if let Some(kind) = kind_filter.as_deref() {
        connection
            .query_row(
                "
                SELECT task_id
                FROM worker_tasks
                WHERE kind = ?1
                  AND next_retry_at_ms <= ?2
                  AND attempt_count < ?3
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?2)
                  )
                ORDER BY created_at_ms ASC
                LIMIT 1
                ",
                params![kind, request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "
                SELECT task_id
                FROM worker_tasks
                WHERE next_retry_at_ms <= ?1
                  AND attempt_count < ?2
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?1)
                  )
                ORDER BY created_at_ms ASC
                LIMIT 1
                ",
                params![request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    };

    let Some(task_id) = row_id else {
        return Ok(None);
    };
    connection.execute(
        "
        UPDATE worker_tasks
        SET state = 'running',
            lease_owner = ?2,
            lease_expires_at_ms = ?3,
            attempt_count = attempt_count + 1,
            updated_at_ms = ?4
        WHERE task_id = ?1
        ",
        params![
            task_id,
            request.lease_owner,
            request.now_ms.saturating_add(request.lease_duration_ms),
            request.now_ms,
        ],
    )?;

    worker_task_by_id(connection, &task_id)
}

pub(super) fn complete_worker_task(
    connection: &Connection,
    request: WorkerTaskCompletion,
) -> Result<WorkerTaskRecord, StorageError> {
    let changed = connection.execute(
        "
        UPDATE worker_tasks
        SET state = 'succeeded',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?4
        WHERE task_id = ?1 AND lease_owner = ?2 AND attempt_count = ?3
        ",
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "worker task completion did not match an active lease".to_owned(),
        ));
    }

    worker_task_by_id_required(connection, &request.task_id)
}

pub(super) fn fail_worker_task(
    connection: &Connection,
    request: WorkerTaskFailure,
) -> Result<WorkerTaskRecord, StorageError> {
    let state = if request.attempt_count >= request.max_attempts {
        WorkerTaskState::DeadLetter
    } else {
        WorkerTaskState::Retrying
    };
    let changed = connection.execute(
        "
        UPDATE worker_tasks
        SET state = ?4,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = ?5,
            last_error_message = ?6,
            next_retry_at_ms = ?7,
            updated_at_ms = ?8
        WHERE task_id = ?1 AND lease_owner = ?2 AND attempt_count = ?3
        ",
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            state.as_str(),
            request.error_kind,
            request.error_message,
            request.now_ms.saturating_add(request.retry_backoff_ms),
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "worker task failure did not match an active lease".to_owned(),
        ));
    }

    worker_task_by_id_required(connection, &request.task_id)
}

pub(super) fn insert_proposal(
    connection: &Connection,
    proposal: NewProposal,
) -> Result<ProposalRecord, StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO proposals (
            proposal_id, source_scope, kind, state, title, summary, payload_json,
            origin, confidence_basis_points, created_at_ms, updated_at_ms
        ) VALUES (?1, ?2, ?3, 'proposed', ?4, ?5, ?6, ?7, ?8, ?9, ?9)
        ",
        params![
            proposal.proposal_id,
            proposal.source_scope,
            proposal.kind.as_str(),
            proposal.title,
            proposal.summary,
            proposal.payload_json,
            proposal.origin,
            proposal.confidence_basis_points,
            proposal.now_ms,
        ],
    )?;
    for conflict in proposal.conflicts {
        insert_proposal_conflict(connection, &proposal.proposal_id, conflict)?;
    }

    proposal_by_id_required(connection, &proposal.proposal_id)
}

pub(super) fn list_proposals(
    connection: &Connection,
    request: ProposalListRequest,
) -> Result<Vec<ProposalRecord>, StorageError> {
    let limit = i64::try_from(request.limit.max(1)).unwrap_or(i64::MAX);
    if let Some(state) = request.state {
        let mut statement = connection.prepare(
            "
            SELECT p.*, COUNT(c.conflict_id) AS conflict_count
            FROM proposals p
            LEFT JOIN proposal_conflicts c ON c.proposal_id = p.proposal_id
            WHERE p.state = ?1
            GROUP BY p.proposal_id
            ORDER BY p.updated_at_ms DESC
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![state.as_str(), limit], proposal_from_row)?;
        return collect_rows(rows);
    }

    let mut statement = connection.prepare(
        "
        SELECT p.*, COUNT(c.conflict_id) AS conflict_count
        FROM proposals p
        LEFT JOIN proposal_conflicts c ON c.proposal_id = p.proposal_id
        GROUP BY p.proposal_id
        ORDER BY p.updated_at_ms DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map(params![limit], proposal_from_row)?;
    collect_rows(rows)
}

pub(super) fn proposal_by_id(
    connection: &Connection,
    proposal_id: &str,
) -> Result<Option<ProposalRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT p.*, COUNT(c.conflict_id) AS conflict_count
            FROM proposals p
            LEFT JOIN proposal_conflicts c ON c.proposal_id = p.proposal_id
            WHERE p.proposal_id = ?1
            GROUP BY p.proposal_id
            ",
            params![proposal_id],
            proposal_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn proposal_conflicts(
    connection: &Connection,
    proposal_id: &str,
) -> Result<Vec<ProposalConflictRecord>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT conflict_id, proposal_id, existing_fact_kind, existing_fact_id, severity, reason
        FROM proposal_conflicts
        WHERE proposal_id = ?1
        ORDER BY severity DESC, conflict_id ASC
        ",
    )?;
    let rows = statement.query_map(params![proposal_id], conflict_from_row)?;
    collect_rows(rows)
}

pub(super) fn decide_proposal(
    connection: &Connection,
    request: ProposalDecision,
) -> Result<ProposalRecord, StorageError> {
    let current = proposal_by_id_required(connection, &request.proposal_id)?;
    if current.state != ProposalState::Proposed {
        return Err(StorageError::InvalidInput(format!(
            "proposal '{}' is already {}",
            current.proposal_id,
            current.state.as_str()
        )));
    }
    connection.execute(
        "
        UPDATE proposals
        SET state = ?2,
            decided_by = ?3,
            decision_reason = ?4,
            updated_at_ms = ?5
        WHERE proposal_id = ?1
        ",
        params![
            request.proposal_id,
            request.next_state.as_str(),
            request.actor,
            request.reason,
            request.now_ms,
        ],
    )?;

    proposal_by_id_required(connection, &request.proposal_id)
}

pub(super) fn insert_audit_event(
    connection: &Connection,
    event: NewAuditEvent,
) -> Result<AuditEventRecord, StorageError> {
    connection.execute(
        "
        INSERT INTO audit_events (
            operation, interface, request_id, trace_id, status, actor, source_scope,
            graph_version, detail_json, message, created_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
        params![
            event.operation,
            event.interface,
            event.request_id,
            event.trace_id,
            event.status.as_str(),
            event.actor,
            event.source_scope,
            event.graph_version,
            event.detail_json,
            event.message,
            event.now_ms,
        ],
    )?;
    let sequence = u64::try_from(connection.last_insert_rowid()).unwrap_or(u64::MAX);

    audit_event_by_sequence(connection, sequence)
}

pub(super) fn query_audit_events(
    connection: &Connection,
    request: AuditQueryRequest,
) -> Result<Vec<AuditEventRecord>, StorageError> {
    let limit = i64::try_from(request.limit.max(1)).unwrap_or(i64::MAX);
    if let Some(operation) = request.operation {
        let mut statement = connection.prepare(
            "
            SELECT sequence, operation, interface, request_id, trace_id, status, actor,
                   source_scope, graph_version, detail_json, message, created_at_ms
            FROM audit_events
            WHERE operation = ?1
            ORDER BY sequence DESC
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![operation, limit], audit_event_from_row)?;
        return collect_rows(rows);
    }

    let mut statement = connection.prepare(
        "
        SELECT sequence, operation, interface, request_id, trace_id, status, actor,
               source_scope, graph_version, detail_json, message, created_at_ms
        FROM audit_events
        ORDER BY sequence DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map(params![limit], audit_event_from_row)?;
    collect_rows(rows)
}

pub(super) fn audit_event_count(connection: &Connection) -> Result<usize, StorageError> {
    let count = connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| {
        row.get::<_, u64>(0)
    })?;

    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

pub(super) fn service_operator_status(
    connection: &Connection,
) -> Result<ServiceOperatorStatus, StorageError> {
    connection
        .query_row(
            "
            SELECT state, silent_updates_enabled, allowed_scopes_json, last_run_at_ms,
                   next_retry_at_ms, last_error, updated_at_ms
            FROM service_operator_state
            WHERE id = 1
            ",
            [],
            service_operator_from_row,
        )
        .map_err(StorageError::from)
}

pub(super) fn update_service_operator(
    connection: &Connection,
    request: ServiceOperatorUpdate,
) -> Result<ServiceOperatorStatus, StorageError> {
    let allowed_scopes_json = serde_json::to_string(&request.allowed_scopes)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        UPDATE service_operator_state
        SET state = ?1,
            silent_updates_enabled = ?2,
            allowed_scopes_json = ?3,
            last_error = ?4,
            updated_at_ms = ?5
        WHERE id = 1
        ",
        params![
            request.state.as_str(),
            request.silent_updates_enabled,
            allowed_scopes_json,
            request.last_error,
            request.now_ms,
        ],
    )?;

    service_operator_status(connection)
}

fn worker_status(connection: &Connection, kind: WorkerKind) -> Result<WorkerStatus, StorageError> {
    let queue_depth = count_worker_state(connection, kind, "queued")?;
    let running_count = count_worker_state(connection, kind, "running")?;
    let retrying_count = count_worker_state(connection, kind, "retrying")?;
    let dead_letter_count = count_worker_state(connection, kind, "dead_letter")?;
    let last_error = connection
        .query_row(
            "
            SELECT last_error_message
            FROM worker_tasks
            WHERE kind = ?1 AND last_error_message IS NOT NULL
            ORDER BY updated_at_ms DESC
            LIMIT 1
            ",
            params![kind.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(WorkerStatus {
        kind,
        backend_state: WorkerBackendState::Fallback,
        endpoint_configured: false,
        queue_depth,
        running_count,
        retrying_count,
        dead_letter_count,
        last_error,
    })
}

fn count_worker_state(
    connection: &Connection,
    kind: WorkerKind,
    state: &str,
) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM worker_tasks WHERE kind = ?1 AND state = ?2",
        params![kind.as_str(), state],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn worker_task_by_kind_fingerprint(
    connection: &Connection,
    kind: WorkerKind,
    task_id: String,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, evidence_id, target_graph_version, state,
                   lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
                   input_fingerprint, payload_json, last_error_kind, last_error_message,
                   created_at_ms, updated_at_ms
            FROM worker_tasks
            WHERE kind = ?1 AND task_id = ?2
            ",
            params![kind.as_str(), task_id],
            worker_task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn worker_task_by_id(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, evidence_id, target_graph_version, state,
                   lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
                   input_fingerprint, payload_json, last_error_kind, last_error_message,
                   created_at_ms, updated_at_ms
            FROM worker_tasks
            WHERE task_id = ?1
            ",
            params![task_id],
            worker_task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn worker_task_by_id_required(
    connection: &Connection,
    task_id: &str,
) -> Result<WorkerTaskRecord, StorageError> {
    worker_task_by_id(connection, task_id)?
        .ok_or_else(|| StorageError::InvalidInput(format!("worker task '{task_id}' not found")))
}

fn worker_task_from_row(row: &Row<'_>) -> rusqlite::Result<WorkerTaskRecord> {
    let kind = parse_worker_kind(row.get::<_, String>(1)?);
    let state = parse_worker_task_state(row.get::<_, String>(5)?);
    Ok(WorkerTaskRecord {
        task_id: row.get(0)?,
        kind,
        source_scope: row.get(2)?,
        evidence_id: row.get(3)?,
        target_graph_version: GraphVersion::new(row.get::<_, u64>(4)?),
        state,
        lease_owner: row.get(6)?,
        lease_expires_at_ms: row.get(7)?,
        attempt_count: row.get(8)?,
        next_retry_at_ms: row.get(9)?,
        input_fingerprint: row.get(10)?,
        payload_json: row.get(11)?,
        last_error_kind: row.get(12)?,
        last_error_message: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
    })
}

fn insert_proposal_conflict(
    connection: &Connection,
    proposal_id: &str,
    conflict: NewProposalConflict,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO proposal_conflicts (
            conflict_id, proposal_id, existing_fact_kind, existing_fact_id, severity, reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![
            conflict.conflict_id,
            proposal_id,
            conflict.existing_fact_kind,
            conflict.existing_fact_id,
            conflict.severity.as_str(),
            conflict.reason,
        ],
    )?;

    Ok(())
}

fn proposal_by_id_required(
    connection: &Connection,
    proposal_id: &str,
) -> Result<ProposalRecord, StorageError> {
    proposal_by_id(connection, proposal_id)?
        .ok_or_else(|| StorageError::InvalidInput(format!("proposal '{proposal_id}' not found")))
}

fn proposal_from_row(row: &Row<'_>) -> rusqlite::Result<ProposalRecord> {
    Ok(ProposalRecord {
        proposal_id: row.get(0)?,
        source_scope: row.get(1)?,
        kind: parse_proposal_kind(row.get::<_, String>(2)?),
        state: parse_proposal_state(row.get::<_, String>(3)?),
        title: row.get(4)?,
        summary: row.get(5)?,
        payload_json: row.get(6)?,
        origin: row.get(7)?,
        confidence_basis_points: row.get(8)?,
        decided_by: row.get(9)?,
        decision_reason: row.get(10)?,
        created_at_ms: row.get(11)?,
        updated_at_ms: row.get(12)?,
        conflict_count: row.get(13)?,
    })
}

fn conflict_from_row(row: &Row<'_>) -> rusqlite::Result<ProposalConflictRecord> {
    Ok(ProposalConflictRecord {
        conflict_id: row.get(0)?,
        proposal_id: row.get(1)?,
        existing_fact_kind: row.get(2)?,
        existing_fact_id: row.get(3)?,
        severity: parse_conflict_severity(row.get::<_, String>(4)?),
        reason: row.get(5)?,
    })
}

fn audit_event_by_sequence(
    connection: &Connection,
    sequence: u64,
) -> Result<AuditEventRecord, StorageError> {
    connection
        .query_row(
            "
            SELECT sequence, operation, interface, request_id, trace_id, status, actor,
                   source_scope, graph_version, detail_json, message, created_at_ms
            FROM audit_events
            WHERE sequence = ?1
            ",
            params![sequence],
            audit_event_from_row,
        )
        .map_err(StorageError::from)
}

fn audit_event_from_row(row: &Row<'_>) -> rusqlite::Result<AuditEventRecord> {
    Ok(AuditEventRecord {
        sequence: row.get(0)?,
        operation: row.get(1)?,
        interface: row.get(2)?,
        request_id: row.get(3)?,
        trace_id: row.get(4)?,
        status: parse_audit_status(row.get::<_, String>(5)?),
        actor: row.get(6)?,
        source_scope: row.get(7)?,
        graph_version: row.get(8)?,
        detail_json: row.get(9)?,
        message: row.get(10)?,
        created_at_ms: row.get(11)?,
    })
}

fn service_operator_from_row(row: &Row<'_>) -> rusqlite::Result<ServiceOperatorStatus> {
    let allowed_scopes_json: String = row.get(2)?;
    let allowed_scopes = serde_json::from_str(&allowed_scopes_json).unwrap_or_default();
    Ok(ServiceOperatorStatus {
        state: parse_service_operator_state(row.get::<_, String>(0)?),
        silent_updates_enabled: row.get(1)?,
        allowed_scopes,
        last_run_at_ms: row.get(3)?,
        next_retry_at_ms: row.get(4)?,
        last_error: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn parse_worker_kind(value: String) -> WorkerKind {
    WorkerKind::parse(&value).unwrap_or(WorkerKind::Extractor)
}

fn parse_worker_task_state(value: String) -> WorkerTaskState {
    WorkerTaskState::parse(&value).unwrap_or(WorkerTaskState::Failed)
}

fn parse_proposal_kind(value: String) -> ProposalKind {
    ProposalKind::parse(&value).unwrap_or(ProposalKind::Evidence)
}

fn parse_proposal_state(value: String) -> ProposalState {
    ProposalState::parse(&value).unwrap_or(ProposalState::Rejected)
}

fn parse_conflict_severity(value: String) -> ProposalConflictSeverity {
    ProposalConflictSeverity::parse(&value).unwrap_or(ProposalConflictSeverity::Warning)
}

fn parse_audit_status(value: String) -> AuditStatus {
    AuditStatus::parse(&value).unwrap_or(AuditStatus::Failed)
}

fn parse_service_operator_state(value: String) -> ServiceOperatorState {
    ServiceOperatorState::parse(&value).unwrap_or(ServiceOperatorState::Failed)
}

fn worker_task_id(kind: WorkerKind, fingerprint: &str) -> String {
    format!(
        "worker:{}:{:016x}",
        kind.as_str(),
        stable_hash64(fingerprint.as_bytes())
    )
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
