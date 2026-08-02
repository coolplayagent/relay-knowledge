use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::{
    domain::RetrieverSource,
    storage::{GraphSearchOutcome, GraphSearchRequest, StorageError},
};

use super::super::{
    advanced,
    bm25::bm25_candidate_rows,
    bm25_fallback,
    context::{evidence_ids_from_bm25_rows, facts_for_evidence_ids, graph_evidence_candidates},
    derived,
    ranking::{Candidate, merge_ranked},
};
use super::{bm25_hit::scored_bm25_hit, candidate::ScoredHit};

pub(in crate::storage::sqlite) fn search_graph(
    connection: &mut Connection,
    request: GraphSearchRequest,
) -> Result<GraphSearchOutcome, StorageError> {
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

    Ok(GraphSearchOutcome::from_hits(&request, hits))
}

fn bm25_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    if let Some(match_query) = fts_query(&request.query) {
        let rows = bm25_candidate_rows(connection, request, &match_query)?;
        if !rows.is_empty() {
            let facts_by_evidence = facts_for_evidence_ids(
                connection,
                evidence_ids_from_bm25_rows(&rows),
                request.graph_version,
            )?;
            let hits: Vec<ScoredHit> = rows
                .into_iter()
                .map(|row| {
                    scored_bm25_hit(connection, row, request.graph_version, &facts_by_evidence)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(hits);
        }
    }

    bm25_fallback::fallback_candidates(connection, request)
}

fn fts_query(query: &str) -> Option<String> {
    let tokens = query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
