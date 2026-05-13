use std::collections::{BTreeMap, BTreeSet};

mod advanced;

use rusqlite::{Connection, Row, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeGraphArtifact, CodeGraphArtifactKind, ConfidenceScore, ContextEntity, ContextGraphFact,
        ContextGraphFactKind, EvidenceExtractionMetadata, EvidenceModality, EvidenceSpan,
        FactStatus, GraphVersion, GraphVersionRange, RECIPROCAL_RANK_FUSION_K, RankingSignal,
        RetrievalHit, RetrieverSource,
    },
    storage::{GraphSearchRequest, StorageError},
};

#[path = "retrieval_migration.rs"]
mod migration;

#[path = "retrieval_aliases.rs"]
mod aliases;

const LABEL_SEPARATOR: char = '\u{1f}';
const FACT_LOOKUP_CHUNK_SIZE: usize = 250;
const LOCAL_SEMANTIC_MODEL: &str = "relay-local-token-semantic-v1";
const LOCAL_VECTOR_MODEL: &str = "relay-local-hash-ann-v1";
const LOCAL_VECTOR_DIMENSION: usize = 16;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    let rebuild_required = migration::drop_incompatible_bm25_table(connection)?;
    connection.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS graph_bm25 USING fts5(
            document_id UNINDEXED,
            document_kind UNINDEXED,
            evidence_id UNINDEXED,
            parent_evidence_id UNINDEXED,
            modality UNINDEXED,
            created_graph_version UNINDEXED,
            source_scope,
            source_path,
            entity_labels,
            entity_aliases,
            content
        );

        CREATE TABLE IF NOT EXISTS graph_semantic_documents (
            document_id TEXT PRIMARY KEY,
            document_kind TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            parent_evidence_id TEXT,
            modality TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            source_scope TEXT NOT NULL,
            source_path TEXT,
            entity_labels_json TEXT NOT NULL,
            content TEXT NOT NULL,
            token_signature_json TEXT NOT NULL,
            model TEXT NOT NULL,
            dimension INTEGER NOT NULL,
            source_hash TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS graph_vector_documents (
            document_id TEXT PRIMARY KEY,
            document_kind TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            parent_evidence_id TEXT,
            modality TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            source_scope TEXT NOT NULL,
            source_path TEXT,
            entity_labels_json TEXT NOT NULL,
            content TEXT NOT NULL,
            vector_json TEXT NOT NULL,
            model TEXT NOT NULL,
            dimension INTEGER NOT NULL,
            source_hash TEXT NOT NULL
        );
        ",
    )?;
    if rebuild_required {
        migration::rebuild_bm25_documents(connection)?;
    }

    Ok(())
}

pub(super) struct EvidenceDocumentInput<'a> {
    pub evidence_id: &'a str,
    pub source_scope: &'a str,
    pub source_path: Option<&'a str>,
    pub entity_labels: &'a [String],
    pub content: &'a str,
    pub status: FactStatus,
    pub extraction: &'a EvidenceExtractionMetadata,
    pub source_hash: &'a str,
    pub graph_version: u64,
}

pub(super) fn replace_evidence_document(
    connection: &Connection,
    input: EvidenceDocumentInput<'_>,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(input.evidence_id);
    connection.execute(
        "DELETE FROM graph_bm25 WHERE document_id = ?1",
        params![document_id],
    )?;
    connection.execute(
        "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
        params![document_id],
    )?;
    connection.execute(
        "DELETE FROM graph_vector_documents WHERE document_id = ?1",
        params![document_id],
    )?;
    if !retrievable_status(input.status) {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'evidence', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
        params![
            document_id,
            input.evidence_id,
            input.extraction.parent_evidence_id.as_deref(),
            input.extraction.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            aliases_from_strings(input.entity_labels),
            input.content,
        ],
    )?;
    replace_semantic_document(
        connection,
        SemanticDocumentInput {
            document_id: &document_id,
            document_kind: "evidence",
            evidence_id: input.evidence_id,
            parent_evidence_id: input.extraction.parent_evidence_id.as_deref(),
            modality: input.extraction.modality,
            source_scope: input.source_scope,
            source_path: input.source_path,
            entity_labels: input.entity_labels,
            content: input.content,
            source_hash: input.source_hash,
            graph_version: input.graph_version,
            model: input
                .extraction
                .embedding_model
                .as_deref()
                .unwrap_or(LOCAL_SEMANTIC_MODEL),
            dimension: input
                .extraction
                .embedding_dimension
                .map(usize::from)
                .unwrap_or(LOCAL_VECTOR_DIMENSION),
        },
    )?;
    replace_vector_document(
        connection,
        VectorDocumentInput {
            document_id: &document_id,
            document_kind: "evidence",
            evidence_id: input.evidence_id,
            parent_evidence_id: input.extraction.parent_evidence_id.as_deref(),
            modality: input.extraction.modality,
            source_scope: input.source_scope,
            source_path: input.source_path,
            entity_labels: input.entity_labels,
            content: input.content,
            source_hash: input.source_hash,
            graph_version: input.graph_version,
            model: input
                .extraction
                .embedding_model
                .as_deref()
                .unwrap_or(LOCAL_VECTOR_MODEL),
            dimension: input
                .extraction
                .embedding_dimension
                .map(usize::from)
                .unwrap_or(LOCAL_VECTOR_DIMENSION),
        },
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
    let entity_aliases = aliases::lexical_aliases(&[name, kind, path, symbol_id]);
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'code_symbol', ?2, NULL, 'text_span', ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            document_id,
            symbol_id,
            graph_version,
            source_scope,
            path,
            join_labels(&[name.to_owned()]),
            entity_aliases,
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
    let linked_symbols = linked_symbol_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let entity_aliases = aliases::lexical_aliases(&linked_symbols);
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'code_chunk', ?2, NULL, 'text_span', ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            document_id,
            chunk_id,
            graph_version,
            source_scope,
            path,
            join_labels(linked_symbol_ids),
            entity_aliases,
            content
        ],
    )?;

    Ok(())
}

struct SemanticDocumentInput<'a> {
    document_id: &'a str,
    document_kind: &'a str,
    evidence_id: &'a str,
    parent_evidence_id: Option<&'a str>,
    modality: EvidenceModality,
    source_scope: &'a str,
    source_path: Option<&'a str>,
    entity_labels: &'a [String],
    content: &'a str,
    source_hash: &'a str,
    graph_version: u64,
    model: &'a str,
    dimension: usize,
}

fn replace_semantic_document(
    connection: &Connection,
    input: SemanticDocumentInput<'_>,
) -> Result<(), StorageError> {
    let signature = token_signature(
        input.content,
        input.entity_labels,
        input.source_path,
        input.source_hash,
    );
    connection.execute(
        "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
        params![input.document_id],
    )?;
    connection.execute(
        "
        INSERT INTO graph_semantic_documents (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, source_scope, source_path, entity_labels_json,
            content, token_signature_json, model, dimension, source_hash
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ",
        params![
            input.document_id,
            input.document_kind,
            input.evidence_id,
            input.parent_evidence_id,
            input.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            input.content,
            json_string_array(&signature)?,
            input.model,
            input.dimension as i64,
            input.source_hash,
        ],
    )?;

    Ok(())
}

struct VectorDocumentInput<'a> {
    document_id: &'a str,
    document_kind: &'a str,
    evidence_id: &'a str,
    parent_evidence_id: Option<&'a str>,
    modality: EvidenceModality,
    source_scope: &'a str,
    source_path: Option<&'a str>,
    entity_labels: &'a [String],
    content: &'a str,
    source_hash: &'a str,
    graph_version: u64,
    model: &'a str,
    dimension: usize,
}

fn replace_vector_document(
    connection: &Connection,
    input: VectorDocumentInput<'_>,
) -> Result<(), StorageError> {
    let vector = hashed_vector(
        input.content,
        input.entity_labels,
        input.source_path,
        input.source_hash,
        input.dimension,
    );
    connection.execute(
        "DELETE FROM graph_vector_documents WHERE document_id = ?1",
        params![input.document_id],
    )?;
    connection.execute(
        "
        INSERT INTO graph_vector_documents (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, source_scope, source_path, entity_labels_json,
            content, vector_json, model, dimension, source_hash
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ",
        params![
            input.document_id,
            input.document_kind,
            input.evidence_id,
            input.parent_evidence_id,
            input.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            input.content,
            json_f64_array(&vector)?,
            input.model,
            input.dimension as i64,
            input.source_hash,
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
    merge_ranked(
        &mut candidates,
        advanced::semantic_candidates(connection, &request)?,
        RetrieverSource::Semantic,
        "local semantic token signature read model with scope and graph-version filters",
    );
    merge_ranked(
        &mut candidates,
        advanced::vector_candidates(connection, &request)?,
        RetrieverSource::Vector,
        "local hashed vector ANN read model with model, dimension, source hash, scope, and graph-version metadata",
    );
    merge_ranked(
        &mut candidates,
        advanced::path_candidates(connection, &request)?,
        RetrieverSource::GraphPath,
        "schema-guided traversal over accepted relations, claims, events, and supporting evidence",
    );
    merge_ranked(
        &mut candidates,
        advanced::temporal_candidates(connection, &request)?,
        RetrieverSource::Temporal,
        "temporal event retrieval using occurred-at and as-of query constraints",
    );
    merge_ranked(
        &mut candidates,
        advanced::community_summary_candidates(connection, &request)?,
        RetrieverSource::CommunitySummary,
        "community summary read model generated from scoped entity and fact neighborhoods",
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
            graph_bm25.parent_evidence_id,
            graph_bm25.modality,
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
                parent_evidence_id: row.get(3)?,
                modality: row.get(4)?,
                source_scope: row.get(5)?,
                source_path: row.get(6)?,
                entity_labels: split_labels(row.get(7)?),
                content: row.get(8)?,
                rank: row.get(9)?,
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
        key: if row.document_kind == "evidence" {
            evidence_group_key(
                row.parent_evidence_id
                    .as_deref()
                    .unwrap_or(&row.evidence_id),
            )
        } else {
            row.document_id
        },
        hit: RetrievalHit {
            evidence_id: row.parent_evidence_id.unwrap_or(row.evidence_id),
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
        modality: row.modality,
        explanation: None,
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
        let explanation_text = scored
            .explanation
            .unwrap_or_else(|| format!("{explanation}; modality={}", scored.modality));
        let candidate = candidates
            .entry(scored.key)
            .and_modify(|candidate| candidate.merge_hit(&scored.hit))
            .or_insert_with(|| Candidate::new(scored.hit));
        if !candidate.hit.retriever_sources.contains(&source) {
            candidate.hit.retriever_sources.push(source);
        }
        candidate.hit.ranking.push(RankingSignal {
            source,
            rank,
            score: scored.source_score,
            explanation: explanation_text,
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

    fn merge_hit(&mut self, hit: &RetrievalHit) {
        if !hit.content.is_empty() && !self.hit.content.contains(&hit.content) {
            if !self.hit.content.is_empty() {
                self.hit.content.push_str("\n\n");
            }
            self.hit.content.push_str(&hit.content);
        }
        if self.hit.source_path.is_none() {
            self.hit.source_path = hit.source_path.clone();
        }
        if self.hit.source_span.is_none() {
            self.hit.source_span = hit.source_span;
        }
        if self.hit.code_artifact.is_none() {
            self.hit.code_artifact = hit.code_artifact.clone();
        }
        for label in &hit.entity_labels {
            if !self.hit.entity_labels.contains(label) {
                self.hit.entity_labels.push(label.clone());
            }
        }
        for entity in &hit.entities {
            if !self
                .hit
                .entities
                .iter()
                .any(|existing| existing.id == entity.id)
            {
                self.hit.entities.push(entity.clone());
            }
        }
        for fact in &hit.graph_facts {
            if !self
                .hit
                .graph_facts
                .iter()
                .any(|existing| existing.fact_id == fact.fact_id && existing.kind == fact.kind)
            {
                self.hit.graph_facts.push(fact.clone());
            }
        }
    }
}

struct ScoredHit {
    key: String,
    hit: RetrievalHit,
    source: RetrieverSource,
    source_score: f64,
    modality: String,
    explanation: Option<String>,
}

struct RawBm25Row {
    document_id: String,
    document_kind: String,
    evidence_id: String,
    parent_evidence_id: Option<String>,
    modality: String,
    source_scope: String,
    source_path: Option<String>,
    entity_labels: Vec<String>,
    content: String,
    rank: f64,
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

fn fts_query(query: &str) -> Option<String> {
    let tokens = query
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

fn token_signature(
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
    source_hash: &str,
) -> Vec<String> {
    let mut terms = BTreeSet::new();
    collect_terms(content, &mut terms);
    collect_terms(&labels.join(" "), &mut terms);
    collect_terms(source_path.unwrap_or_default(), &mut terms);
    collect_terms(source_hash, &mut terms);

    terms.into_iter().collect()
}

fn collect_terms(value: &str, terms: &mut BTreeSet<String>) {
    for token in value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| token.len() >= 2)
    {
        terms.insert(token.to_ascii_lowercase());
    }
}

fn hashed_vector(
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
    source_hash: &str,
    dimension: usize,
) -> Vec<f64> {
    if dimension == 0 {
        return Vec::new();
    }
    let terms = token_signature(content, labels, source_path, source_hash);
    let mut vector = vec![0.0; dimension];
    for term in terms {
        let hash = stable_hash64(term.as_bytes());
        let index = (hash as usize) % dimension;
        let sign = if hash & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    normalize_vector(&mut vector);

    vector
}

fn normalize_vector(vector: &mut [f64]) {
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}

fn semantic_overlap_score(
    query_terms: &BTreeSet<String>,
    document_terms: &BTreeSet<String>,
) -> f64 {
    if query_terms.is_empty() || document_terms.is_empty() {
        return 0.0;
    }
    let intersection = query_terms.intersection(document_terms).count();
    if intersection == 0 {
        return 0.0;
    }
    let union = query_terms.union(document_terms).count();

    intersection as f64 / query_terms.len() as f64 + intersection as f64 / union as f64
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .max(0.0)
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

fn evidence_group_key(evidence_id: &str) -> String {
    format!("evidence_group:{evidence_id}")
}

fn code_document_id(kind: &str, source_scope: &str, path: &str, id: &str) -> String {
    format!("code:{kind}:{source_scope}:{path}:{id}")
}

fn join_labels(labels: &[String]) -> String {
    serde_json::to_string(labels).unwrap_or_default()
}

fn json_string_array(values: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn json_f64_array(values: &[f64]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn parse_string_array(value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn parse_f64_array(value: &str) -> Result<Vec<f64>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn sort_scored_hits(hits: &mut [ScoredHit]) {
    hits.sort_by(|left, right| {
        right
            .source_score
            .total_cmp(&left.source_score)
            .then_with(|| left.hit.evidence_id.cmp(&right.hit.evidence_id))
    });
}

fn aliases_from_strings(values: &[String]) -> String {
    let values = values.iter().map(String::as_str).collect::<Vec<_>>();
    aliases::lexical_aliases(&values)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_signature_and_vector_are_deterministic() {
        let labels = vec!["Rust".to_owned()];
        let signature = token_signature("Async Rust graph", &labels, Some("src/lib.rs"), "abc");
        let first = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), "abc", 8);
        let second = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), "abc", 8);

        assert!(signature.contains(&"rust".to_owned()));
        assert_eq!(first, second);
        assert!((cosine_similarity(&first, &second) - 1.0).abs() < 0.000_001);
    }
}
