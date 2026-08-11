use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, ffi::ErrorCode};

use crate::{
    domain::RetrieverSource,
    storage::{
        GraphSearchOutcome, GraphSearchRequest, MAX_GRAPH_SEARCH_FTS_CODEPOINTS,
        MAX_GRAPH_SEARCH_FTS_TOKENS, MAX_GRAPH_SEARCH_LIMIT, MAX_GRAPH_SEARCH_QUERY_CHARS,
        MAX_GRAPH_SEARCH_TOKEN_BYTES, StorageError,
    },
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
    validate_search_request(&request)?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let outcome = search_graph_snapshot(&transaction, request)?;
    transaction.commit()?;
    Ok(outcome)
}

fn validate_search_request(request: &GraphSearchRequest) -> Result<(), StorageError> {
    if request.limit == 0 {
        return Err(StorageError::InvalidInput(
            "search limit must be greater than zero".to_owned(),
        ));
    }
    if request.limit > MAX_GRAPH_SEARCH_LIMIT {
        return Err(StorageError::InvalidInput(format!(
            "search limit must not exceed {MAX_GRAPH_SEARCH_LIMIT}"
        )));
    }
    if request.query.chars().count() > MAX_GRAPH_SEARCH_QUERY_CHARS {
        return Err(StorageError::InvalidInput(format!(
            "search query must not exceed {MAX_GRAPH_SEARCH_QUERY_CHARS} characters"
        )));
    }
    let mut fts_token_count = 0_usize;
    let mut fts_codepoint_count = 0_usize;
    for token in request
        .query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        fts_token_count = fts_token_count.saturating_add(1);
        if fts_token_count > MAX_GRAPH_SEARCH_FTS_TOKENS {
            return Err(StorageError::InvalidInput(format!(
                "search query must not exceed {MAX_GRAPH_SEARCH_FTS_TOKENS} lexical tokens"
            )));
        }
        if token.len() > MAX_GRAPH_SEARCH_TOKEN_BYTES {
            return Err(StorageError::InvalidInput(format!(
                "search lexical tokens must not exceed {MAX_GRAPH_SEARCH_TOKEN_BYTES} UTF-8 bytes"
            )));
        }
        fts_codepoint_count = fts_codepoint_count.saturating_add(token.chars().count());
        if fts_codepoint_count > MAX_GRAPH_SEARCH_FTS_CODEPOINTS {
            return Err(StorageError::InvalidInput(format!(
                "search lexical input must not exceed {MAX_GRAPH_SEARCH_FTS_CODEPOINTS} Unicode code points"
            )));
        }
    }
    Ok(())
}

fn search_graph_snapshot(
    connection: &Connection,
    request: GraphSearchRequest,
) -> Result<GraphSearchOutcome, StorageError> {
    let mut candidates = BTreeMap::new();
    let companion_generation_available = !derived_generation_is_building(connection)?;
    let bm25_requested = request.allows_retriever_source(RetrieverSource::Bm25)
        || request.allows_retriever_source(RetrieverSource::CodeGraph);
    let mut bm25_outcome = if bm25_requested {
        bm25_candidates(connection, &request, companion_generation_available)?
    } else {
        Bm25CandidateOutcome {
            hits: Vec::new(),
            degraded_reason: None,
        }
    };
    bm25_outcome
        .hits
        .retain(|hit| request.allows_retriever_source(hit.source));
    let mut degraded_reason = bm25_outcome.degraded_reason;
    merge_ranked(
        &mut candidates,
        bm25_outcome.hits,
        RetrieverSource::Bm25,
        "fts5 bm25 over evidence, entity labels, source paths, code symbols, and code chunks",
    );
    if request.allows_retriever_source(RetrieverSource::GraphEvidence) {
        merge_ranked(
            &mut candidates,
            graph_evidence_candidates(connection, &request)?,
            RetrieverSource::GraphEvidence,
            "term overlap over graph evidence and entity labels",
        );
    }
    if companion_generation_available && request.allows_retriever_source(RetrieverSource::Semantic)
    {
        merge_ranked(
            &mut candidates,
            derived::semantic_candidates(connection, &request)?,
            RetrieverSource::Semantic,
            "local semantic token signature read model with scope and graph-version filters",
        );
    }
    if companion_generation_available && request.allows_retriever_source(RetrieverSource::Vector) {
        merge_ranked(
            &mut candidates,
            derived::vector_candidates(connection, &request)?,
            RetrieverSource::Vector,
            "local hashed vector ANN read model with model, dimension, source hash, scope, and graph-version metadata",
        );
    }
    if request.allows_retriever_source(RetrieverSource::GraphPath) {
        merge_ranked(
            &mut candidates,
            advanced::path_candidates(connection, &request)?,
            RetrieverSource::GraphPath,
            "schema-guided traversal over accepted relations, claims, events, and supporting evidence",
        );
    }
    if request.allows_retriever_source(RetrieverSource::Temporal) {
        merge_ranked(
            &mut candidates,
            advanced::temporal_candidates(connection, &request)?,
            RetrieverSource::Temporal,
            "temporal event retrieval using occurred-at and as-of query constraints",
        );
    }
    if request.allows_retriever_source(RetrieverSource::CommunitySummary) {
        merge_ranked(
            &mut candidates,
            advanced::community_summary_candidates(connection, &request)?,
            RetrieverSource::CommunitySummary,
            "community summary read model generated from scoped entity and fact neighborhoods",
        );
    }
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

    let mut outcome = GraphSearchOutcome::from_hits(&request, hits);
    let paused_source_requested = bm25_requested
        || request.allows_retriever_source(RetrieverSource::Semantic)
        || request.allows_retriever_source(RetrieverSource::Vector);
    if !companion_generation_available && paused_source_requested {
        let maintenance_reason = "derived-index rebuild in progress; semantic, vector, and lexical fallback retrievers paused";
        degraded_reason = Some(match degraded_reason {
            Some(reason) => format!("{reason}; {maintenance_reason}"),
            None => maintenance_reason.to_owned(),
        });
    }
    outcome.trace.degraded_reason = degraded_reason;
    Ok(outcome)
}

fn derived_generation_is_building(connection: &Connection) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT state = 'building' FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map(|building| building.unwrap_or(false))
        .map_err(StorageError::from)
}

struct Bm25CandidateOutcome {
    hits: Vec<ScoredHit>,
    degraded_reason: Option<String>,
}

fn bm25_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
    companion_generation_available: bool,
) -> Result<Bm25CandidateOutcome, StorageError> {
    if let Some(match_query) = fts_query(&request.query) {
        let rows = match bm25_candidate_rows(connection, request, &match_query) {
            Ok(rows) => rows,
            Err(error) if bm25_source_is_temporarily_unavailable(&error) => {
                return Ok(Bm25CandidateOutcome {
                    hits: Vec::new(),
                    degraded_reason: Some(
                        "bm25 temporarily unavailable; other retrievers continued".to_owned(),
                    ),
                });
            }
            Err(error) => return Err(error),
        };
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
            return Ok(Bm25CandidateOutcome {
                hits,
                degraded_reason: None,
            });
        }
    }

    if !companion_generation_available {
        return Ok(Bm25CandidateOutcome {
            hits: Vec::new(),
            degraded_reason: None,
        });
    }
    let fallback = bm25_fallback::fallback_candidates(connection, request)?;
    Ok(Bm25CandidateOutcome {
        hits: fallback.hits,
        degraded_reason: fallback.degraded_reason,
    })
}

fn bm25_source_is_temporarily_unavailable(error: &StorageError) -> bool {
    match error {
        StorageError::Busy(_) => true,
        StorageError::Sqlite(error) => {
            let transient_code = matches!(
                error,
                rusqlite::Error::SqliteFailure(inner, _)
                    if matches!(
                        inner.code,
                        ErrorCode::DatabaseBusy
                            | ErrorCode::DatabaseLocked
                            | ErrorCode::SchemaChanged
                    )
            );
            transient_code || super::super::graph_bm25_transient_error_message(&error.to_string())
        }
        _ => false,
    }
}

fn fts_query(query: &str) -> Option<String> {
    let tokens = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
