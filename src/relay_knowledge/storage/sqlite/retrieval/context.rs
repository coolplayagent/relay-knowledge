use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Row, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeGraphArtifact, CodeGraphArtifactKind, ConfidenceScore, ContextEntity, ContextGraphFact,
        ContextGraphFactKind, EvidenceSpan, FactStatus, GraphVersion, GraphVersionRange,
        RetrievalHit, RetrieverSource,
    },
    storage::{GraphSearchRequest, StorageError},
};

use super::{RawBm25Row, ScoredHit, evidence_group_key, overlap_score};

const FACT_LOOKUP_CHUNK_SIZE: usize = 250;

pub(super) fn evidence_span(
    connection: &Connection,
    evidence_id: &str,
    graph_version: GraphVersion,
) -> Result<Option<EvidenceSpan>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT span_start_byte, span_end_byte, span_start_line, span_end_line
        FROM evidence
        WHERE id = ?1 AND created_graph_version <= ?2
        ",
    )?;
    let mut rows = statement.query(params![evidence_id, graph_version.get()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    optional_span_from_row(row, 0)
}

pub(super) fn code_artifact_for_document(
    document_kind: &str,
    artifact_id: &str,
    path: Option<&str>,
) -> Option<CodeGraphArtifact> {
    let kind = match document_kind {
        "code_symbol" => CodeGraphArtifactKind::Symbol,
        "code_chunk" => CodeGraphArtifactKind::Chunk,
        _ => return None,
    };

    Some(CodeGraphArtifact {
        kind,
        artifact_id: artifact_id.to_owned(),
        path: path.unwrap_or_default().to_owned(),
    })
}

pub(super) fn graph_evidence_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT
            e.id,
            e.parent_evidence_id,
            e.modality,
            e.source_scope,
            e.source_path,
            e.span_start_byte,
            e.span_end_byte,
            e.span_start_line,
            e.span_end_line,
            e.content
	        FROM evidence e
	        WHERE (?1 IS NULL OR e.source_scope = ?1)
	          AND e.created_graph_version <= ?2
	          AND e.status IN ('accepted', 'proposed')
	        ORDER BY e.created_graph_version DESC, e.id ASC
	        ",
    )?;
    let rows = statement.query_map(
        params![request.source_scope.as_deref(), request.graph_version.get()],
        |row| {
            Ok(RawEvidenceRow {
                evidence_id: row.get(0)?,
                parent_evidence_id: row.get(1)?,
                modality: row.get(2)?,
                source_scope: row.get(3)?,
                source_path: row.get(4)?,
                span_start_byte: row.get(5)?,
                span_end_byte: row.get(6)?,
                span_start_line: row.get(7)?,
                span_end_line: row.get(8)?,
                content: row.get(9)?,
            })
        },
    )?;
    let evidence_rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut hits = Vec::new();
    let mut fact_evidence_ids_by_group = BTreeMap::<String, Vec<String>>::new();
    for row in evidence_rows {
        let source_span = optional_span_from_parts(
            row.span_start_byte,
            row.span_end_byte,
            row.span_start_line,
            row.span_end_line,
        )?;
        let entities = entities_for_evidence(connection, &row.evidence_id)?;
        let entity_labels = entities
            .iter()
            .map(|entity| entity.label.clone())
            .collect::<Vec<_>>();
        let score = overlap_score(
            &request.query,
            &row.content,
            &entity_labels,
            row.source_path.as_deref(),
        );
        if score > 0.0 {
            let group_id = row
                .parent_evidence_id
                .as_deref()
                .unwrap_or(row.evidence_id.as_str());
            fact_evidence_ids_by_group
                .entry(group_id.to_owned())
                .or_default()
                .push(row.evidence_id.clone());
            hits.push(ScoredHit {
                key: evidence_group_key(group_id),
                hit: RetrievalHit {
                    evidence_id: group_id.to_owned(),
                    source_scope: row.source_scope,
                    source_path: row.source_path,
                    source_span,
                    content: row.content,
                    entity_labels,
                    entities,
                    graph_facts: Vec::new(),
                    code_artifact: None,
                    retriever_sources: Vec::new(),
                    ranking: Vec::new(),
                    rerank: None,
                    score: 0.0,
                },
                source: RetrieverSource::GraphEvidence,
                source_score: score,
                modality: row.modality,
                explanation: None,
            });
        }
    }
    hits.sort_by(|left, right| {
        right
            .source_score
            .total_cmp(&left.source_score)
            .then_with(|| left.hit.evidence_id.cmp(&right.hit.evidence_id))
    });
    hits.truncate(request.limit.saturating_mul(4).max(request.limit));
    let facts_by_evidence = facts_for_evidence_ids(
        connection,
        fact_evidence_ids_by_group
            .values()
            .flat_map(|evidence_ids| evidence_ids.iter().cloned())
            .collect::<Vec<_>>(),
        request.graph_version,
    )?;
    for scored in &mut hits {
        let Some(evidence_ids) = fact_evidence_ids_by_group.get(&scored.hit.evidence_id) else {
            continue;
        };
        for evidence_id in evidence_ids {
            if let Some(facts) = facts_by_evidence.get(evidence_id) {
                for fact in facts {
                    if !scored.hit.graph_facts.iter().any(|existing| {
                        existing.fact_id == fact.fact_id && existing.kind == fact.kind
                    }) {
                        scored.hit.graph_facts.push(fact.clone());
                    }
                }
            }
        }
    }

    Ok(hits)
}

pub(super) fn entities_for_evidence(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Vec<ContextEntity>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.id, ent.label
        FROM evidence_entities ee
        INNER JOIN entities ent ON ent.id = ee.entity_id
        WHERE ee.evidence_id = ?1
        ORDER BY ent.label ASC, ent.id ASC
        ",
    )?;
    let rows = statement.query_map(params![evidence_id], |row| {
        Ok(ContextEntity {
            id: row.get(0)?,
            label: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn facts_for_evidence_ids(
    connection: &Connection,
    evidence_ids: Vec<String>,
    graph_version: GraphVersion,
) -> Result<BTreeMap<String, Vec<ContextGraphFact>>, StorageError> {
    let evidence_ids = evidence_ids.into_iter().collect::<BTreeSet<_>>();
    if evidence_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut facts = BTreeMap::new();
    let evidence_ids = evidence_ids.into_iter().collect::<Vec<_>>();
    for chunk in evidence_ids.chunks(FACT_LOOKUP_CHUNK_SIZE) {
        let chunk_ids = chunk.iter().cloned().collect::<BTreeSet<_>>();
        extend_relation_facts(connection, &chunk_ids, graph_version, &mut facts)?;
        extend_claim_facts(connection, &chunk_ids, graph_version, &mut facts)?;
        extend_event_facts(connection, &chunk_ids, graph_version, &mut facts)?;
    }

    Ok(facts)
}

pub(super) fn evidence_ids_from_bm25_rows(rows: &[RawBm25Row]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.document_kind == "evidence")
        .map(|row| row.evidence_id.clone())
        .collect()
}

pub(super) fn retrievable_status(status: FactStatus) -> bool {
    matches!(status, FactStatus::Accepted | FactStatus::Proposed)
}

fn extend_relation_facts(
    connection: &Connection,
    evidence_ids: &BTreeSet<String>,
    graph_version: GraphVersion,
    facts: &mut BTreeMap<String, Vec<ContextGraphFact>>,
) -> Result<(), StorageError> {
    let sql = format!(
        "
        SELECT l.evidence_id, r.id, src.label, r.relation_type, tgt.label, r.evidence_ids_json,
               r.confidence_basis_points, r.status, r.valid_from_graph_version,
               r.valid_until_graph_version
        FROM graph_fact_evidence l
        INNER JOIN graph_relations r ON r.id = l.fact_id AND l.fact_kind = 'relation'
        INNER JOIN entities src ON src.id = r.source_entity_id
        INNER JOIN entities tgt ON tgt.id = r.target_entity_id
        WHERE r.created_graph_version <= ?1
          AND r.valid_from_graph_version <= ?1
          AND (r.valid_until_graph_version IS NULL OR r.valid_until_graph_version >= ?1)
          AND l.evidence_id IN ({})
        ORDER BY l.evidence_id ASC, r.id ASC
        ",
        placeholders(evidence_ids.len())
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fact_query_params(graph_version, evidence_ids)),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u16>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, u64>(8)?,
                row.get::<_, Option<u64>>(9)?,
            ))
        },
    )?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    for (
        linked_evidence_id,
        id,
        source,
        relation_type,
        target,
        evidence_json,
        confidence,
        status,
        from,
        until,
    ) in rows
    {
        facts
            .entry(linked_evidence_id)
            .or_default()
            .push(ContextGraphFact {
                fact_id: id,
                kind: ContextGraphFactKind::Relation,
                subject: source,
                predicate: relation_type,
                object: Some(target),
                evidence_ids: evidence_ids_from_json(&evidence_json)?,
                confidence: ConfidenceScore {
                    basis_points: confidence,
                },
                status: parse_fact_status(&status)?,
                version_range: version_range(from, until)?,
            });
    }

    Ok(())
}

fn extend_claim_facts(
    connection: &Connection,
    evidence_ids: &BTreeSet<String>,
    graph_version: GraphVersion,
    facts: &mut BTreeMap<String, Vec<ContextGraphFact>>,
) -> Result<(), StorageError> {
    let sql = format!(
        "
        SELECT l.evidence_id, c.id, ent.label, c.predicate, c.object, c.evidence_ids_json,
               c.confidence_basis_points, c.status, c.valid_from_graph_version,
               c.valid_until_graph_version
        FROM graph_fact_evidence l
        INNER JOIN graph_claims c ON c.id = l.fact_id AND l.fact_kind = 'claim'
        INNER JOIN entities ent ON ent.id = c.subject_entity_id
        WHERE c.created_graph_version <= ?1
          AND c.valid_from_graph_version <= ?1
          AND (c.valid_until_graph_version IS NULL OR c.valid_until_graph_version >= ?1)
          AND l.evidence_id IN ({})
        ORDER BY l.evidence_id ASC, c.id ASC
        ",
        placeholders(evidence_ids.len())
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fact_query_params(graph_version, evidence_ids)),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u16>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, u64>(8)?,
                row.get::<_, Option<u64>>(9)?,
            ))
        },
    )?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    for (
        linked_evidence_id,
        id,
        subject,
        predicate,
        object,
        evidence_json,
        confidence,
        status,
        from,
        until,
    ) in rows
    {
        facts
            .entry(linked_evidence_id)
            .or_default()
            .push(ContextGraphFact {
                fact_id: id,
                kind: ContextGraphFactKind::Claim,
                subject,
                predicate,
                object: Some(object),
                evidence_ids: evidence_ids_from_json(&evidence_json)?,
                confidence: ConfidenceScore {
                    basis_points: confidence,
                },
                status: parse_fact_status(&status)?,
                version_range: version_range(from, until)?,
            });
    }

    Ok(())
}

fn extend_event_facts(
    connection: &Connection,
    evidence_ids: &BTreeSet<String>,
    graph_version: GraphVersion,
    facts: &mut BTreeMap<String, Vec<ContextGraphFact>>,
) -> Result<(), StorageError> {
    let sql = format!(
        "
        SELECT l.evidence_id, e.id, e.event_type, e.occurred_at, e.evidence_ids_json,
               e.confidence_basis_points, e.status, e.valid_from_graph_version,
               e.valid_until_graph_version
        FROM graph_fact_evidence l
        INNER JOIN graph_events e ON e.id = l.fact_id AND l.fact_kind = 'event'
        WHERE e.created_graph_version <= ?1
          AND e.valid_from_graph_version <= ?1
          AND (e.valid_until_graph_version IS NULL OR e.valid_until_graph_version >= ?1)
          AND l.evidence_id IN ({})
        ORDER BY l.evidence_id ASC, e.id ASC
        ",
        placeholders(evidence_ids.len())
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fact_query_params(graph_version, evidence_ids)),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u16>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, u64>(7)?,
                row.get::<_, Option<u64>>(8)?,
            ))
        },
    )?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    for (
        linked_evidence_id,
        id,
        event_type,
        occurred_at,
        evidence_json,
        confidence,
        status,
        from,
        until,
    ) in rows
    {
        facts
            .entry(linked_evidence_id)
            .or_default()
            .push(ContextGraphFact {
                subject: event_entity_labels(connection, &id)?.join(", "),
                fact_id: id,
                kind: ContextGraphFactKind::Event,
                predicate: event_type,
                object: occurred_at,
                evidence_ids: evidence_ids_from_json(&evidence_json)?,
                confidence: ConfidenceScore {
                    basis_points: confidence,
                },
                status: parse_fact_status(&status)?,
                version_range: version_range(from, until)?,
            });
    }

    Ok(())
}

fn event_entity_labels(
    connection: &Connection,
    event_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.label
        FROM graph_event_entities gee
        INNER JOIN entities ent ON ent.id = gee.entity_id
        WHERE gee.event_id = ?1
        ORDER BY ent.label ASC
        ",
    )?;
    let rows = statement.query_map(params![event_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn fact_query_params(graph_version: GraphVersion, evidence_ids: &BTreeSet<String>) -> Vec<Value> {
    let mut values = Vec::with_capacity(evidence_ids.len() + 1);
    values.push(Value::Integer(graph_version.get() as i64));
    values.extend(evidence_ids.iter().cloned().map(Value::Text));
    values
}

fn evidence_ids_from_json(value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(super) fn parse_fact_status(value: &str) -> Result<FactStatus, StorageError> {
    FactStatus::parse(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn version_range(
    valid_from: u64,
    valid_until: Option<u64>,
) -> Result<GraphVersionRange, StorageError> {
    GraphVersionRange::new(
        GraphVersion::new(valid_from),
        valid_until.map(GraphVersion::new),
    )
    .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn optional_span_from_row(
    row: &Row<'_>,
    start: usize,
) -> Result<Option<EvidenceSpan>, StorageError> {
    optional_span_from_parts(
        row.get(start)?,
        row.get(start + 1)?,
        row.get(start + 2)?,
        row.get(start + 3)?,
    )
}

fn optional_span_from_parts(
    start_byte: Option<u32>,
    end_byte: Option<u32>,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> Result<Option<EvidenceSpan>, StorageError> {
    match (start_byte, end_byte, start_line, end_line) {
        (Some(start_byte), Some(end_byte), Some(start_line), Some(end_line)) => {
            EvidenceSpan::new(start_byte, end_byte, start_line, end_line)
                .map(Some)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))
        }
        _ => Ok(None),
    }
}

struct RawEvidenceRow {
    evidence_id: String,
    parent_evidence_id: Option<String>,
    modality: String,
    source_scope: String,
    source_path: Option<String>,
    span_start_byte: Option<u32>,
    span_end_byte: Option<u32>,
    span_start_line: Option<u32>,
    span_end_line: Option<u32>,
    content: String,
}
