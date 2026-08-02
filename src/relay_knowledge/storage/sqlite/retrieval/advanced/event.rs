use rusqlite::{Connection, params};

use crate::{
    domain::{ConfidenceScore, ContextGraphFact, ContextGraphFactKind},
    storage::{GraphSearchRequest, StorageError},
};

use super::support::SupportContext;
use crate::storage::sqlite::retrieval::context::{parse_fact_status, version_range};

pub(super) struct EventRow {
    pub(super) id: String,
    pub(super) event_type: String,
    pub(super) occurred_at: Option<String>,
    pub(super) evidence_ids_json: String,
    confidence: u16,
    status: String,
    valid_from_graph_version: u64,
    valid_until_graph_version: Option<u64>,
    pub(super) labels: String,
}

impl EventRow {
    pub(super) fn graph_fact(
        &self,
        context: &SupportContext,
    ) -> Result<ContextGraphFact, StorageError> {
        Ok(ContextGraphFact {
            fact_id: self.id.clone(),
            kind: ContextGraphFactKind::Event,
            subject: self.labels.clone(),
            predicate: self.event_type.clone(),
            object: self.occurred_at.clone(),
            evidence_ids: context.evidence_ids.clone(),
            confidence: ConfidenceScore {
                basis_points: self.confidence,
            },
            status: parse_fact_status(&self.status)?,
            version_range: version_range(
                self.valid_from_graph_version,
                self.valid_until_graph_version,
            )?,
        })
    }
}

pub(super) fn load_events(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<EventRow>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ge.id, ge.event_type, ge.occurred_at, ge.evidence_ids_json,
               ge.confidence_basis_points, ge.status, ge.valid_from_graph_version,
               ge.valid_until_graph_version, group_concat(ent.label, ' ')
        FROM graph_events ge
        INNER JOIN graph_event_entities gee ON gee.event_id = ge.id
        INNER JOIN entities ent ON ent.id = gee.entity_id
        WHERE ge.status = 'accepted'
          AND ge.created_graph_version <= ?1
          AND ge.valid_from_graph_version <= ?1
          AND (ge.valid_until_graph_version IS NULL OR ge.valid_until_graph_version >= ?1)
        GROUP BY ge.id, ge.event_type, ge.occurred_at, ge.evidence_ids_json,
                 ge.confidence_basis_points, ge.status, ge.valid_from_graph_version,
                 ge.valid_until_graph_version
        ORDER BY ge.occurred_at DESC, ge.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok(EventRow {
            id: row.get(0)?,
            event_type: row.get(1)?,
            occurred_at: row.get(2)?,
            evidence_ids_json: row.get(3)?,
            confidence: row.get(4)?,
            status: row.get(5)?,
            valid_from_graph_version: row.get(6)?,
            valid_until_graph_version: row.get(7)?,
            labels: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn occurred_label(occurred_at: Option<&str>) -> String {
    occurred_at
        .map(|value| format!(" at {value}"))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
