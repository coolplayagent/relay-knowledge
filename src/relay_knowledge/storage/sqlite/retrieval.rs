use std::collections::{BTreeMap, BTreeSet};

mod advanced;

use rusqlite::{Connection, params};

use crate::{
    domain::{
        ContextGraphFact, EvidenceExtractionMetadata, EvidenceModality, FactStatus, GraphVersion,
        RetrievalHit, RetrieverSource,
    },
    retrieval::terms::extend_normalized_terms,
    storage::{GraphSearchRequest, StorageError},
};

#[path = "retrieval/context.rs"]
mod context;

#[path = "retrieval/derived.rs"]
mod derived;

#[path = "retrieval_migration.rs"]
mod migration;

#[path = "retrieval_aliases.rs"]
mod aliases;

#[path = "retrieval/ranking.rs"]
mod ranking;

use context::{
    code_artifact_for_document, entities_for_evidence, evidence_ids_from_bm25_rows, evidence_span,
    facts_for_evidence_ids, graph_evidence_candidates, retrievable_status,
};
use ranking::{Candidate, merge_ranked};

const LABEL_SEPARATOR: char = '\u{1f}';
const LOCAL_SEMANTIC_MODEL: &str = "relay-local-token-semantic-v1";
const LOCAL_VECTOR_MODEL: &str = "relay-local-hash-ann-v1";
pub(super) const LOCAL_TOKENIZER_VERSION: &str = "relay-normalized-terms-v2";
const LOCAL_VECTOR_DIMENSION: usize = 16;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
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
            source_hash TEXT NOT NULL,
            tokenizer_version TEXT NOT NULL
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
            source_hash TEXT NOT NULL,
            tokenizer_version TEXT NOT NULL
        );
        ",
    )?;
    if migration::derived_documents_missing(connection)? {
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
    connection.execute(
        "
        DELETE FROM graph_semantic_documents
        WHERE document_kind IN ('code_symbol', 'code_chunk')
          AND source_scope = ?1
          AND source_path = ?2
        ",
        params![source_scope, path],
    )?;
    connection.execute(
        "
        DELETE FROM graph_vector_documents
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
    let labels = [name.to_owned()];
    let entity_aliases = aliases::lexical_aliases(&[name, kind, path, symbol_id]);
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
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
            join_labels(&labels),
            entity_aliases,
            content
        ],
    )?;
    replace_semantic_document(
        connection,
        SemanticDocumentInput {
            document_id: &document_id,
            document_kind: "code_symbol",
            evidence_id: symbol_id,
            parent_evidence_id: None,
            modality: EvidenceModality::TextSpan,
            source_scope,
            source_path: Some(path),
            entity_labels: &labels,
            content: &content,
            source_hash: &source_hash,
            graph_version,
            model: LOCAL_SEMANTIC_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;
    replace_vector_document(
        connection,
        VectorDocumentInput {
            document_id: &document_id,
            document_kind: "code_symbol",
            evidence_id: symbol_id,
            parent_evidence_id: None,
            modality: EvidenceModality::TextSpan,
            source_scope,
            source_path: Some(path),
            entity_labels: &labels,
            content: &content,
            source_hash: &source_hash,
            graph_version,
            model: LOCAL_VECTOR_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
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
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
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
    replace_semantic_document(
        connection,
        SemanticDocumentInput {
            document_id: &document_id,
            document_kind: "code_chunk",
            evidence_id: chunk_id,
            parent_evidence_id: None,
            modality: EvidenceModality::TextSpan,
            source_scope,
            source_path: Some(path),
            entity_labels: linked_symbol_ids,
            content,
            source_hash: &source_hash,
            graph_version,
            model: LOCAL_SEMANTIC_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;
    replace_vector_document(
        connection,
        VectorDocumentInput {
            document_id: &document_id,
            document_kind: "code_chunk",
            evidence_id: chunk_id,
            parent_evidence_id: None,
            modality: EvidenceModality::TextSpan,
            source_scope,
            source_path: Some(path),
            entity_labels: linked_symbol_ids,
            content,
            source_hash: &source_hash,
            graph_version,
            model: LOCAL_VECTOR_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
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
    let signature = token_signature(input.content, input.entity_labels, input.source_path);
    connection.execute(
        "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
        params![input.document_id],
    )?;
    connection.execute(
        "
        INSERT INTO graph_semantic_documents (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, source_scope, source_path, entity_labels_json,
            content, token_signature_json, model, dimension, source_hash, tokenizer_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
            LOCAL_TOKENIZER_VERSION,
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
            content, vector_json, model, dimension, source_hash, tokenizer_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
            LOCAL_TOKENIZER_VERSION,
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
    if request.allows_retriever_source(RetrieverSource::Semantic) {
        merge_ranked(
            &mut candidates,
            derived::semantic_candidates(connection, &request)?,
            RetrieverSource::Semantic,
            "local semantic token signature read model with scope and graph-version filters",
        );
    }
    if request.allows_retriever_source(RetrieverSource::Vector) {
        merge_ranked(
            &mut candidates,
            derived::vector_candidates(connection, &request)?,
            RetrieverSource::Vector,
            "local hashed vector ANN read model with model, dimension, source hash, scope, and graph-version metadata",
        );
    }
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
            rerank: None,
            score: 0.0,
        },
        source,
        source_score: -row.rank,
        modality: row.modality,
        explanation: None,
    })
}

pub(super) struct ScoredHit {
    pub(super) key: String,
    pub(super) hit: RetrievalHit,
    pub(super) source: RetrieverSource,
    pub(super) source_score: f64,
    pub(super) modality: String,
    pub(super) explanation: Option<String>,
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

fn fts_query(query: &str) -> Option<String> {
    let tokens = query
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

fn token_signature(content: &str, labels: &[String], source_path: Option<&str>) -> Vec<String> {
    let mut terms = BTreeSet::new();
    collect_terms(content, &mut terms);
    collect_terms(&labels.join(" "), &mut terms);
    collect_terms(source_path.unwrap_or_default(), &mut terms);

    terms.into_iter().collect()
}

fn collect_terms(value: &str, terms: &mut BTreeSet<String>) {
    extend_normalized_terms(value, 2, terms);
}

fn hashed_vector(
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
    dimension: usize,
) -> Vec<f64> {
    if dimension == 0 {
        return Vec::new();
    }
    let terms = token_signature(content, labels, source_path);
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
    if score > 0.0 {
        return score;
    }

    identifier_overlap_score(query, content, labels, source_path)
}

fn identifier_overlap_score(
    query: &str,
    content: &str,
    labels: &[String],
    source_path: Option<&str>,
) -> f64 {
    let query_terms = token_signature(query, &[], None);
    let document_terms = token_signature(content, labels, source_path);
    query_terms
        .iter()
        .filter(|term| {
            let term = term.as_str();
            document_terms
                .iter()
                .any(|candidate| candidate == term || fuzzy_identifier_part_match(term, candidate))
        })
        .count() as f64
}

fn fuzzy_identifier_part_match(query_term: &str, candidate: &str) -> bool {
    query_term.len() >= 3 && candidate.len() >= 3 && candidate.contains(query_term)
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

pub(super) fn join_labels(labels: &[String]) -> String {
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

pub(super) fn split_labels(labels: String) -> Vec<String> {
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
        let signature = token_signature("Async Rust graph", &labels, Some("src/lib.rs"));
        let first = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), 8);
        let second = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), 8);

        assert!(signature.contains(&"rust".to_owned()));
        assert_eq!(first, second);
        assert!((cosine_similarity(&first, &second) - 1.0).abs() < 0.000_001);
    }

    #[test]
    fn token_signature_adds_identifier_parts_for_semantic_and_vector_recall() {
        let labels = vec!["SemanticVectorRecall".to_owned()];
        let signature = token_signature("GraphRAGContextPack", &labels, None);

        for term in [
            "semantic", "vector", "recall", "graph", "rag", "context", "pack",
        ] {
            assert!(signature.contains(&term.to_owned()), "missing term {term}");
        }
    }

    #[test]
    fn semantic_document_stores_source_hash_without_retrieval_token_noise() {
        let connection = Connection::open_in_memory().expect("database should open");
        connection
            .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
            .expect("evidence table should exist for retrieval migration checks");
        initialize_schema(&connection).expect("schema should initialize");
        let labels = vec!["SemanticVectorRecall".to_owned()];
        replace_semantic_document(
            &connection,
            SemanticDocumentInput {
                document_id: "doc",
                document_kind: "evidence",
                evidence_id: "ev",
                parent_evidence_id: None,
                modality: EvidenceModality::TextSpan,
                source_scope: "scope",
                source_path: Some("docs/source.md"),
                entity_labels: &labels,
                content: "backend freshness source attribution",
                source_hash: "sha256:abcdef123456",
                graph_version: 1,
                model: LOCAL_SEMANTIC_MODEL,
                dimension: LOCAL_VECTOR_DIMENSION,
            },
        )
        .expect("semantic document should insert");
        let (signature_json, source_hash): (String, String) = connection
            .query_row(
                "
                SELECT token_signature_json, source_hash
                FROM graph_semantic_documents
                WHERE document_id = 'doc'
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("semantic row should load");
        let signature = parse_string_array(&signature_json).expect("signature should parse");

        assert_eq!(source_hash, "sha256:abcdef123456");
        assert!(signature.contains(&"backend".to_owned()));
        assert!(signature.contains(&"semantic".to_owned()));
        assert!(signature.contains(&"source".to_owned()));
        assert!(!signature.contains(&"sha256".to_owned()));
        assert!(!signature.contains(&"abcdef123456".to_owned()));
    }
}
