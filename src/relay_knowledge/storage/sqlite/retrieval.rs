use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::{RECIPROCAL_RANK_FUSION_K, RankingSignal, RetrievalHit, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

const LABEL_SEPARATOR: char = '\u{1f}';

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    drop_incompatible_bm25_table(connection)?;
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

    Ok(())
}

fn drop_incompatible_bm25_table(connection: &Connection) -> Result<(), StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = 'graph_bm25'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(());
    }

    let mut statement = connection.prepare("PRAGMA table_info(graph_bm25)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    if !columns
        .iter()
        .any(|column| column == "created_graph_version")
    {
        connection.execute("DROP TABLE graph_bm25", [])?;
    }

    Ok(())
}

pub(super) fn replace_evidence_document(
    transaction: &rusqlite::Transaction<'_>,
    evidence_id: &str,
    source_scope: &str,
    source_path: Option<&str>,
    entity_labels: &[String],
    content: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(evidence_id);
    transaction.execute(
        "DELETE FROM graph_bm25 WHERE document_id = ?1",
        params![document_id],
    )?;
    transaction.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, created_graph_version,
            source_scope, source_path, entity_labels, content
        )
        VALUES (?1, 'evidence', ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            document_id,
            evidence_id,
            graph_version,
            source_scope,
            source_path,
            join_labels(entity_labels),
            content,
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
    let content = format!("{name} {kind} {path}");
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
            document_id,
            document_kind,
            evidence_id,
            source_scope,
            source_path,
            entity_labels,
            content,
            bm25(graph_bm25) AS rank
        FROM graph_bm25
        WHERE graph_bm25 MATCH ?1
          AND (?2 IS NULL OR source_scope = ?2)
          AND created_graph_version <= ?3
        ORDER BY rank ASC, document_id ASC
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
            let document_kind: String = row.get(1)?;
            let source = match document_kind.as_str() {
                "code_symbol" | "code_chunk" => RetrieverSource::CodeGraph,
                _ => RetrieverSource::Bm25,
            };
            let rank = row.get::<_, f64>(7)?;
            Ok(ScoredHit {
                key: row.get(0)?,
                hit: RetrievalHit {
                    evidence_id: row.get(2)?,
                    source_scope: row.get(3)?,
                    source_path: row.get(4)?,
                    entity_labels: split_labels(row.get(5)?),
                    content: row.get(6)?,
                    retriever_sources: Vec::new(),
                    ranking: Vec::new(),
                    score: 0.0,
                },
                source,
                source_score: -rank,
            })
        },
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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
            e.content
        FROM evidence e
        WHERE (?1 IS NULL OR e.source_scope = ?1)
          AND e.created_graph_version <= ?2
        ORDER BY e.created_graph_version DESC, e.id ASC
        ",
    )?;
    let rows = statement.query_map(
        params![request.source_scope.as_deref(), request.graph_version.get()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    let evidence_rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    let mut hits = Vec::new();
    for (evidence_id, source_scope, source_path, content) in evidence_rows {
        let entity_labels = entity_labels_for_evidence(connection, &evidence_id)?;
        let score = overlap_score(
            &request.query,
            &content,
            &entity_labels,
            source_path.as_deref(),
        );
        if score > 0.0 {
            hits.push(ScoredHit {
                key: evidence_document_id(&evidence_id),
                hit: RetrievalHit {
                    evidence_id,
                    source_scope,
                    source_path,
                    content,
                    entity_labels,
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

    Ok(hits)
}

fn entity_labels_for_evidence(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.label
        FROM evidence_entities ee
        INNER JOIN entities ent ON ent.id = ee.entity_id
        WHERE ee.evidence_id = ?1
        ORDER BY ent.label ASC, ent.id ASC
        ",
    )?;
    let rows = statement.query_map(params![evidence_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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
