use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Row, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeGraphArtifact, CodeGraphArtifactKind, ConfidenceScore, ContextEntity, ContextGraphFact,
        ContextGraphFactKind, EvidenceSpan, FactStatus, GraphVersion, GraphVersionRange,
        RECIPROCAL_RANK_FUSION_K, RankingSignal, RetrievalHit, RetrieverSource,
    },
    storage::{GraphSearchRequest, StorageError},
};

#[path = "retrieval_migration.rs"]
mod migration;

const LABEL_SEPARATOR: char = '\u{1f}';
const FACT_LOOKUP_CHUNK_SIZE: usize = 250;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    let rebuild_required = migration::drop_incompatible_bm25_table(connection)?;
    connection.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS graph_bm25 USING fts5(
            document_id UNINDEXED,
            document_kind UNINDEXED,
            evidence_id UNINDEXED,
            created_graph_version UNINDEXED,
            source_scope,
            source_path,
            entity_labels,
            content
        );
        ",
    )?;
    if rebuild_required {
        migration::rebuild_bm25_documents(connection)?;
    }

    Ok(())
}

pub(super) struct EvidenceDocument<'a> {
    pub evidence_id: &'a str,
    pub source_scope: &'a str,
    pub source_path: Option<&'a str>,
    pub entity_labels: &'a [String],
    pub content: &'a str,
    pub status: FactStatus,
    pub graph_version: u64,
}

pub(super) fn replace_evidence_document(
    transaction: &rusqlite::Transaction<'_>,
    document: EvidenceDocument<'_>,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(document.evidence_id);
    transaction.execute(
        "DELETE FROM graph_bm25 WHERE document_id = ?1",
        params![document_id],
    )?;
    insert_evidence_document(transaction, document)
}

fn insert_evidence_document(
    connection: &Connection,
    document: EvidenceDocument<'_>,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(document.evidence_id);
    if !retrievable_status(document.status) {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, created_graph_version,
            source_scope, source_path, entity_labels, content
        )
        VALUES (?1, 'evidence', ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            document_id,
            document.evidence_id,
            document.graph_version,
            document.source_scope,
            document.source_path,
            join_labels(document.entity_labels),
            document.content,
        ],
    )?;

    Ok(())
}

pub(super) fn delete_code_documents(
    connection: &Connection,
    source_scope: &str,
    path: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        DELETE FROM graph_bm25
        WHERE document_kind IN ('code_symbol', 'code_chunk')
          AND source_scope = ?1
          AND source_path = ?2
        ",
        params![source_scope, path],
    )?;

    Ok(())
}

pub(super) fn insert_code_symbol_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    symbol_id: &str,
    name: &str,
    kind: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let document_id = code_document_id("symbol", source_scope, path, symbol_id);
    let content = format!("{name} {kind} {path} {symbol_id}");
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, created_graph_version,
            source_scope, source_path, entity_labels, content
        )
        VALUES (?1, 'code_symbol', ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            document_id,
            symbol_id,
            graph_version,
            source_scope,
            path,
            join_labels(&[name.to_owned()]),
            content
        ],
    )?;

    Ok(())
}

pub(super) fn insert_code_chunk_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    chunk_id: &str,
    linked_symbol_ids: &[String],
    content: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let document_id = code_document_id("chunk", source_scope, path, chunk_id);
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, created_graph_version,
            source_scope, source_path, entity_labels, content
        )
        VALUES (?1, 'code_chunk', ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            document_id,
            chunk_id,
            graph_version,
            source_scope,
            path,
            join_labels(linked_symbol_ids),
            content
        ],
    )?;

    Ok(())
}

pub(super) fn search_graph(
    connection: &mut Connection,
    request: GraphSearchRequest,
) -> Result<Vec<RetrievalHit>, StorageError> {
    if request.limit == 0 {
        return Err(StorageError::InvalidInput(
            "search limit must be greater than zero".to_owned(),
        ));
    }

    let mut candidates = BTreeMap::new();
    merge_ranked(
        &mut candidates,
        bm25_candidates(connection, &request)?,
        RetrieverSource::Bm25,
        "fts5 bm25 over evidence, entity labels, source paths, code symbols, and code chunks",
    );
    merge_ranked(
        &mut candidates,
        graph_evidence_candidates(connection, &request)?,
        RetrieverSource::GraphEvidence,
        "term overlap over graph evidence and entity labels",
    );
    let mut hits = candidates
        .into_values()
        .map(Candidate::into_hit)
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });
    hits.truncate(request.limit);

    Ok(hits)
}

fn bm25_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let Some(match_query) = fts_query(&request.query) else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        "
	        SELECT
	            graph_bm25.document_id,
	            graph_bm25.document_kind,
	            graph_bm25.evidence_id,
	            graph_bm25.source_scope,
	            graph_bm25.source_path,
	            graph_bm25.entity_labels,
	            graph_bm25.content,
	            bm25(graph_bm25) AS rank
	        FROM graph_bm25
	        LEFT JOIN evidence e
	          ON graph_bm25.document_kind = 'evidence'
	         AND e.id = graph_bm25.evidence_id
	        WHERE graph_bm25 MATCH ?1
	          AND (?2 IS NULL OR graph_bm25.source_scope = ?2)
	          AND graph_bm25.created_graph_version <= ?3
	          AND (
	              graph_bm25.document_kind != 'evidence'
	              OR e.status IN ('accepted', 'proposed')
	          )
	        ORDER BY rank ASC, graph_bm25.document_id ASC
	        LIMIT ?4
	        ",
    )?;
    let rows = statement.query_map(
        params![
            match_query,
            request.source_scope.as_deref(),
            request.graph_version.get(),
            request.limit.saturating_mul(4).max(request.limit)
        ],
        |row| {
            Ok(RawBm25Row {
                document_id: row.get(0)?,
                document_kind: row.get(1)?,
                evidence_id: row.get(2)?,
                source_scope: row.get(3)?,
                source_path: row.get(4)?,
                entity_labels: split_labels(row.get(5)?),
                content: row.get(6)?,
                rank: row.get(7)?,
            })
        },
    )?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let facts_by_evidence = facts_for_evidence_ids(
        connection,
        evidence_ids_from_bm25_rows(&rows),
        request.graph_version,
    )?;
    rows.into_iter()
        .map(|row| scored_bm25_hit(connection, row, request.graph_version, &facts_by_evidence))
        .collect()
}

fn scored_bm25_hit(
    connection: &Connection,
    row: RawBm25Row,
    graph_version: GraphVersion,
    facts_by_evidence: &BTreeMap<String, Vec<ContextGraphFact>>,
) -> Result<ScoredHit, StorageError> {
    let source = match row.document_kind.as_str() {
        "code_symbol" | "code_chunk" => RetrieverSource::CodeGraph,
        _ => RetrieverSource::Bm25,
    };
    let (source_span, entities, graph_facts) = if row.document_kind == "evidence" {
        let entities = entities_for_evidence(connection, &row.evidence_id)?;
        let graph_facts = facts_by_evidence
            .get(&row.evidence_id)
            .cloned()
            .unwrap_or_default();
        (
            evidence_span(connection, &row.evidence_id, graph_version)?,
            entities,
            graph_facts,
        )
    } else {
        (None, Vec::new(), Vec::new())
    };
    let code_artifact = code_artifact_for_document(
        &row.document_kind,
        &row.evidence_id,
        row.source_path.as_deref(),
    );
    let entity_labels = if entities.is_empty() {
        row.entity_labels
    } else {
        entities
            .iter()
            .map(|entity| entity.label.clone())
            .collect::<Vec<_>>()
    };

    Ok(ScoredHit {
        key: row.document_id,
        hit: RetrievalHit {
            evidence_id: row.evidence_id,
            source_scope: row.source_scope,
            source_path: row.source_path,
            source_span,
            content: row.content,
            entity_labels,
            entities,
            graph_facts,
            code_artifact,
            retriever_sources: Vec::new(),
            ranking: Vec::new(),
            score: 0.0,
        },
        source,
        source_score: -row.rank,
    })
}

fn evidence_span(
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

fn code_artifact_for_document(
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

fn graph_evidence_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT
            e.id,
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
                source_scope: row.get(1)?,
                source_path: row.get(2)?,
                span_start_byte: row.get(3)?,
                span_end_byte: row.get(4)?,
                span_start_line: row.get(5)?,
                span_end_line: row.get(6)?,
                content: row.get(7)?,
            })
        },
    )?;
    let evidence_rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut hits = Vec::new();
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
            hits.push(ScoredHit {
                key: evidence_document_id(&row.evidence_id),
                hit: RetrievalHit {
                    evidence_id: row.evidence_id,
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
                    score: 0.0,
                },
                source: RetrieverSource::GraphEvidence,
                source_score: score,
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
        hits.iter()
            .map(|hit| hit.hit.evidence_id.clone())
            .collect::<Vec<_>>(),
        request.graph_version,
    )?;
    for scored in &mut hits {
        scored.hit.graph_facts = facts_by_evidence
            .get(&scored.hit.evidence_id)
            .cloned()
            .unwrap_or_default();
    }

    Ok(hits)
}

fn entities_for_evidence(
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

fn facts_for_evidence_ids(
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

fn evidence_ids_from_bm25_rows(rows: &[RawBm25Row]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.document_kind == "evidence")
        .map(|row| row.evidence_id.clone())
        .collect()
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

fn parse_fact_status(value: &str) -> Result<FactStatus, StorageError> {
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

fn retrievable_status(status: FactStatus) -> bool {
    matches!(status, FactStatus::Accepted | FactStatus::Proposed)
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

fn merge_ranked(
    candidates: &mut BTreeMap<String, Candidate>,
    hits: Vec<ScoredHit>,
    fallback_source: RetrieverSource,
    explanation: &'static str,
) {
    for (index, scored) in hits.into_iter().enumerate() {
        let rank = index + 1;
        let rrf_score = 1.0 / (RECIPROCAL_RANK_FUSION_K + rank as f64);
        let source = match scored.source {
            RetrieverSource::CodeGraph => RetrieverSource::CodeGraph,
            _ => fallback_source,
        };
        let candidate = candidates
            .entry(scored.key)
            .or_insert_with(|| Candidate::new(scored.hit));
        if !candidate.hit.retriever_sources.contains(&source) {
            candidate.hit.retriever_sources.push(source);
        }
        candidate.hit.ranking.push(RankingSignal {
            source,
            rank,
            score: scored.source_score,
            explanation: explanation.to_owned(),
        });
        candidate.rrf_score += rrf_score;
    }
}

struct Candidate {
    hit: RetrievalHit,
    rrf_score: f64,
}

impl Candidate {
    fn new(hit: RetrievalHit) -> Self {
        Self {
            hit,
            rrf_score: 0.0,
        }
    }

    fn into_hit(mut self) -> RetrievalHit {
        self.hit.score = self.rrf_score;
        self.hit
    }
}

struct ScoredHit {
    key: String,
    hit: RetrievalHit,
    source: RetrieverSource,
    source_score: f64,
}

struct RawBm25Row {
    document_id: String,
    document_kind: String,
    evidence_id: String,
    source_scope: String,
    source_path: Option<String>,
    entity_labels: Vec<String>,
    content: String,
    rank: f64,
}

struct RawEvidenceRow {
    evidence_id: String,
    source_scope: String,
    source_path: Option<String>,
    span_start_byte: Option<u32>,
    span_end_byte: Option<u32>,
    span_start_line: Option<u32>,
    span_end_line: Option<u32>,
    content: String,
}

fn fts_query(query: &str) -> Option<String> {
    let tokens = query
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

fn overlap_score(query: &str, content: &str, labels: &[String], source_path: Option<&str>) -> f64 {
    let haystack = format!(
        "{} {} {}",
        content.to_lowercase(),
        labels.join(" ").to_lowercase(),
        source_path.unwrap_or_default().to_lowercase()
    );
    let mut score = 0.0;
    for token in query.to_lowercase().split_whitespace() {
        if haystack.contains(token) {
            score += 1.0;
        }
    }

    score
}

fn evidence_document_id(evidence_id: &str) -> String {
    format!("evidence:{evidence_id}")
}

fn code_document_id(kind: &str, source_scope: &str, path: &str, id: &str) -> String {
    format!("code:{kind}:{source_scope}:{path}:{id}")
}

fn join_labels(labels: &[String]) -> String {
    serde_json::to_string(labels).unwrap_or_default()
}

fn split_labels(labels: String) -> Vec<String> {
    serde_json::from_str(&labels).unwrap_or_else(|_| {
        labels
            .split(LABEL_SEPARATOR)
            .filter(|label| !label.is_empty())
            .map(str::to_owned)
            .collect()
    })
}
