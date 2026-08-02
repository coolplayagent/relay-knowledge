//! Durable proposal lifecycle, conflict persistence, and row decoding.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    domain::{
        ProposalConflictRecord, ProposalConflictSeverity, ProposalKind, ProposalProvenance,
        ProposalRecord, ProposalState,
    },
    storage::{
        NewProposal, NewProposalConflict, ProposalDecision, ProposalListRequest, StorageError,
    },
};

pub(in crate::storage::sqlite) fn insert_proposal(
    connection: &Connection,
    proposal: NewProposal,
) -> Result<ProposalRecord, StorageError> {
    let provenance_json = proposal
        .provenance
        .validate()
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?
        .to_json();
    connection.execute(
        "
        INSERT OR IGNORE INTO proposals (
            proposal_id, source_scope, kind, state, title, summary, payload_json,
            origin, provenance_json, confidence_basis_points, created_at_ms, updated_at_ms
        ) VALUES (?1, ?2, ?3, 'proposed', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
        ",
        params![
            proposal.proposal_id,
            proposal.source_scope,
            proposal.kind.as_str(),
            proposal.title,
            proposal.summary,
            proposal.payload_json,
            proposal.origin,
            provenance_json,
            proposal.confidence_basis_points,
            proposal.now_ms,
        ],
    )?;
    for conflict in proposal.conflicts {
        insert_proposal_conflict(connection, &proposal.proposal_id, conflict)?;
    }

    proposal_by_id_required(connection, &proposal.proposal_id)
}

pub(in crate::storage::sqlite) fn list_proposals(
    connection: &Connection,
    request: ProposalListRequest,
) -> Result<Vec<ProposalRecord>, StorageError> {
    let limit = i64::try_from(request.limit.max(1)).unwrap_or(i64::MAX);
    if let Some(state) = request.state {
        let mut statement = connection.prepare(
            "
            SELECT p.proposal_id, p.source_scope, p.kind, p.state, p.title, p.summary,
                   p.payload_json, p.origin, p.provenance_json,
                   p.confidence_basis_points, p.decided_by, p.decision_reason,
                   p.created_at_ms, p.updated_at_ms,
                   COUNT(c.conflict_id) AS conflict_count
            FROM proposals p
            LEFT JOIN proposal_conflicts c ON c.proposal_id = p.proposal_id
            WHERE p.state = ?1
            GROUP BY p.proposal_id
            ORDER BY p.updated_at_ms DESC
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![state.as_str(), limit], proposal_from_row)?;
        return rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from);
    }

    let mut statement = connection.prepare(
        "
        SELECT p.proposal_id, p.source_scope, p.kind, p.state, p.title, p.summary,
               p.payload_json, p.origin, p.provenance_json,
               p.confidence_basis_points, p.decided_by, p.decision_reason,
               p.created_at_ms, p.updated_at_ms,
               COUNT(c.conflict_id) AS conflict_count
        FROM proposals p
        LEFT JOIN proposal_conflicts c ON c.proposal_id = p.proposal_id
        GROUP BY p.proposal_id
        ORDER BY p.updated_at_ms DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map(params![limit], proposal_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn proposal_count(
    connection: &Connection,
    state: Option<ProposalState>,
) -> Result<usize, StorageError> {
    let count = if let Some(state) = state {
        connection.query_row(
            "SELECT COUNT(*) FROM proposals WHERE state = ?1",
            params![state.as_str()],
            |row| row.get::<_, u64>(0),
        )?
    } else {
        connection.query_row("SELECT COUNT(*) FROM proposals", [], |row| {
            row.get::<_, u64>(0)
        })?
    };

    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

pub(in crate::storage::sqlite) fn proposal_by_id(
    connection: &Connection,
    proposal_id: &str,
) -> Result<Option<ProposalRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT p.proposal_id, p.source_scope, p.kind, p.state, p.title, p.summary,
                   p.payload_json, p.origin, p.provenance_json,
                   p.confidence_basis_points, p.decided_by, p.decision_reason,
                   p.created_at_ms, p.updated_at_ms,
                   COUNT(c.conflict_id) AS conflict_count
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

pub(in crate::storage::sqlite) fn proposal_conflicts(
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
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn decide_proposal(
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
        provenance: ProposalProvenance::from_json(&row.get::<_, String>(8)?).unwrap_or_default(),
        confidence_basis_points: row.get(9)?,
        decided_by: row.get(10)?,
        decision_reason: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
        conflict_count: row.get(14)?,
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

fn parse_proposal_kind(value: String) -> ProposalKind {
    ProposalKind::parse(&value).unwrap_or(ProposalKind::Evidence)
}

fn parse_proposal_state(value: String) -> ProposalState {
    ProposalState::parse(&value).unwrap_or(ProposalState::Rejected)
}

fn parse_conflict_severity(value: String) -> ProposalConflictSeverity {
    ProposalConflictSeverity::parse(&value).unwrap_or(ProposalConflictSeverity::Warning)
}

#[cfg(test)]
#[path = "proposals_tests.rs"]
mod tests;
