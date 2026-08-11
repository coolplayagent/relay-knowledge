use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, Params, Statement, ffi::ErrorCode, params};

use crate::storage::{GraphSearchRequest, StorageError};

use super::{ROUTING_ALGORITHM_VERSION, terms};

const MIN_HIERARCHICAL_DOCUMENTS: usize = 4_096;
const MIN_ROUTING_GROUPS: usize = 8;
const MAX_ROUTING_GROUPS: usize = 2_048;
const MAX_GROUP_DOCUMENTS: usize = 512;
const MAX_SELECTED_GROUPS: usize = 32;
const MAX_SELECTED_DOCUMENT_PERCENT: usize = 25;
const HIGH_FREQUENCY_TERM_PERCENT: usize = 20;
const MAX_TERM_VALIDATION_POSTINGS: usize = 65_536;
const MIN_COARSE_SCORE_MARGIN_PERCENT: f64 = 5.0;

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::retrieval) struct Bm25RoutingPlan {
    pub(in crate::storage::sqlite::retrieval) route_match: Option<String>,
    pub(in crate::storage::sqlite::retrieval) explanation: Option<String>,
}

impl Bm25RoutingPlan {
    pub(in crate::storage::sqlite::retrieval) fn flat(reason: &'static str) -> Self {
        Self {
            route_match: None,
            explanation: Some(format!("hierarchical_bm25 fallback={reason}")),
        }
    }
}

#[derive(Clone)]
struct RoutingGroup {
    source_scope: String,
    token: String,
    document_count: usize,
}

#[derive(Default)]
struct GroupScore {
    score: f64,
    matched_terms: usize,
}

pub(in crate::storage::sqlite::retrieval) fn plan_query(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Bm25RoutingPlan, StorageError> {
    match try_plan_query(connection, request) {
        Ok(plan) => Ok(plan),
        Err(error) if routing_state_is_temporarily_unavailable(&error) => {
            Ok(Bm25RoutingPlan::flat("routing_state_unavailable"))
        }
        Err(error) => Err(StorageError::Sqlite(error)),
    }
}

fn routing_state_is_temporarily_unavailable(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(
                inner.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked | ErrorCode::SchemaChanged
            )
    )
}

fn try_plan_query(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> rusqlite::Result<Bm25RoutingPlan> {
    let Some(query_terms) = terms::query_terms(&request.query) else {
        return Ok(Bm25RoutingPlan::flat("query_not_routable"));
    };
    let Some(global_document_count) = route_generation_document_count(connection, request)? else {
        return Ok(Bm25RoutingPlan::flat("stale_route_generation"));
    };
    let groups = load_groups(connection, request.source_scope.as_deref())?;
    let population = groups
        .iter()
        .map(|group| group.document_count)
        .sum::<usize>();
    if (request.source_scope.is_none() && population != global_document_count)
        || population > global_document_count
    {
        return Ok(Bm25RoutingPlan::flat("incomplete_route_index"));
    }
    if !population_is_routable(population, &groups) {
        return Ok(Bm25RoutingPlan::flat("population_guard"));
    }
    let group_keys = groups
        .iter()
        .map(|group| (group.source_scope.clone(), group.token.clone()))
        .collect::<BTreeSet<_>>();

    let mut scores = BTreeMap::<(String, String), GroupScore>::new();
    let mut validation_postings = 0_usize;
    for term in query_terms {
        let routed_df = routed_term_document_frequency(connection, &term)?;
        if routed_df.saturating_mul(100)
            > global_document_count.saturating_mul(HIGH_FREQUENCY_TERM_PERCENT)
        {
            return Ok(Bm25RoutingPlan::flat("low_selectivity"));
        }
        let Some(next_validation_postings) =
            reserve_term_validation(validation_postings, routed_df)
        else {
            return Ok(Bm25RoutingPlan::flat("term_validation_budget"));
        };
        if !business_term_document_frequency_matches(connection, &term, routed_df)? {
            return Ok(Bm25RoutingPlan::flat("incomplete_term_statistics"));
        }
        validation_postings = next_validation_postings;
        if routed_df == 0 {
            continue;
        }
        let idf = global_idf(global_document_count, routed_df);
        if !accumulate_group_scores(
            connection,
            request.source_scope.as_deref(),
            &term,
            idf,
            &group_keys,
            &mut scores,
        )? {
            return Ok(Bm25RoutingPlan::flat("incomplete_group_statistics"));
        }
    }
    if scores.is_empty() {
        return Ok(Bm25RoutingPlan::flat("low_selectivity"));
    }

    Ok(select_groups(population, &groups, scores).unwrap_or_else(Bm25RoutingPlan::flat))
}

fn route_generation_document_count(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> rusqlite::Result<Option<usize>> {
    let current_graph_version = connection
        .query_row(
            "SELECT graph_version FROM graph_state WHERE id = 1",
            [],
            |row| row.get::<_, u64>(0),
        )
        .optional()?;
    let state = connection
        .query_row(
            "SELECT indexed_graph_version, document_count, state, algorithm_version
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(match (current_graph_version, state) {
        (Some(current), Some((indexed, document_count, state, algorithm)))
            if current == request.graph_version.get()
                && indexed == current
                && state == "fresh"
                && algorithm == ROUTING_ALGORITHM_VERSION =>
        {
            Some(document_count)
        }
        _ => None,
    })
}

fn load_groups(
    connection: &Connection,
    source_scope: Option<&str>,
) -> rusqlite::Result<Vec<RoutingGroup>> {
    let limit = MAX_ROUTING_GROUPS.saturating_add(1);
    if let Some(source_scope) = source_scope {
        let mut statement = connection.prepare(
            "SELECT source_scope, group_token, document_count
             FROM graph_bm25_route_groups
             WHERE source_scope = ?1
             ORDER BY group_token
             LIMIT ?2",
        )?;
        return collect_groups(&mut statement, params![source_scope, limit]);
    }
    let mut statement = connection.prepare(
        "SELECT source_scope, group_token, document_count
         FROM graph_bm25_route_groups
         ORDER BY source_scope, group_token
         LIMIT ?1",
    )?;
    collect_groups(&mut statement, params![limit])
}

fn collect_groups<P: Params>(
    statement: &mut Statement<'_>,
    parameters: P,
) -> rusqlite::Result<Vec<RoutingGroup>> {
    statement
        .query_map(parameters, |row| {
            Ok(RoutingGroup {
                source_scope: row.get(0)?,
                token: row.get(1)?,
                document_count: row.get(2)?,
            })
        })?
        .collect()
}

fn population_is_routable(population: usize, groups: &[RoutingGroup]) -> bool {
    if population < MIN_HIERARCHICAL_DOCUMENTS
        || groups.len() < MIN_ROUTING_GROUPS
        || groups.len() > MAX_ROUTING_GROUPS
    {
        return false;
    }
    let mean_ceiling = population.div_ceil(groups.len());
    let skew_limit = mean_ceiling.saturating_mul(2).max(64);
    groups.iter().all(|group| {
        group.document_count <= MAX_GROUP_DOCUMENTS && group.document_count <= skew_limit
    })
}

fn reserve_term_validation(used: usize, expected_document_frequency: usize) -> Option<usize> {
    let required = expected_document_frequency.saturating_add(1);
    let reserved = used.saturating_add(required);
    (reserved <= MAX_TERM_VALIDATION_POSTINGS).then_some(reserved)
}

fn business_term_document_frequency_matches(
    connection: &Connection,
    term: &str,
    expected_document_frequency: usize,
) -> rusqlite::Result<bool> {
    let match_query =
        format!("{{source_scope source_path entity_labels entity_aliases content}} : (\"{term}\")");
    let observed = connection.query_row(
        "SELECT COUNT(*) FROM (
             SELECT rowid FROM graph_bm25
             WHERE graph_bm25 MATCH ?1
             LIMIT ?2
         )",
        params![match_query, expected_document_frequency.saturating_add(1)],
        |row| row.get::<_, usize>(0),
    )?;
    Ok(observed == expected_document_frequency)
}

fn routed_term_document_frequency(connection: &Connection, term: &str) -> rusqlite::Result<usize> {
    Ok(connection
        .query_row(
            "SELECT document_frequency
             FROM graph_bm25_route_term_totals
             WHERE term = ?1",
            params![term],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn global_idf(document_count: usize, document_frequency: usize) -> f64 {
    let numerator = document_count.saturating_sub(document_frequency) as f64 + 0.5;
    (1.0 + numerator / (document_frequency as f64 + 0.5)).ln()
}

fn accumulate_group_scores(
    connection: &Connection,
    source_scope: Option<&str>,
    term: &str,
    idf: f64,
    group_keys: &BTreeSet<(String, String)>,
    scores: &mut BTreeMap<(String, String), GroupScore>,
) -> rusqlite::Result<bool> {
    let limit = MAX_ROUTING_GROUPS.saturating_add(1);
    if let Some(source_scope) = source_scope {
        let mut statement = connection.prepare(
            "SELECT source_scope, group_token, collection_frequency
             FROM graph_bm25_route_terms
             WHERE term = ?1 AND source_scope = ?2
             LIMIT ?3",
        )?;
        return accumulate_score_rows(
            &mut statement,
            params![term, source_scope, limit],
            idf,
            group_keys,
            scores,
        );
    }
    let mut statement = connection.prepare(
        "SELECT source_scope, group_token, collection_frequency
         FROM graph_bm25_route_terms
         WHERE term = ?1
         LIMIT ?2",
    )?;
    accumulate_score_rows(
        &mut statement,
        params![term, limit],
        idf,
        group_keys,
        scores,
    )
}

fn accumulate_score_rows<P: Params>(
    statement: &mut Statement<'_>,
    parameters: P,
    idf: f64,
    group_keys: &BTreeSet<(String, String)>,
    scores: &mut BTreeMap<(String, String), GroupScore>,
) -> rusqlite::Result<bool> {
    let rows = statement.query_map(parameters, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, usize>(2)?,
        ))
    })?;
    let mut row_count = 0_usize;
    for row in rows {
        row_count = row_count.saturating_add(1);
        if row_count > MAX_ROUTING_GROUPS {
            return Ok(false);
        }
        let (scope, token, collection_frequency) = row?;
        let key = (scope, token);
        if !group_keys.contains(&key) {
            return Ok(false);
        }
        let aggregate_weight = (collection_frequency as f64 + 1.0).log2().powi(2);
        let score = scores.entry(key).or_default();
        score.score += idf * aggregate_weight;
        score.matched_terms += 1;
    }
    Ok(true)
}

fn select_groups(
    population: usize,
    groups: &[RoutingGroup],
    scores: BTreeMap<(String, String), GroupScore>,
) -> Result<Bm25RoutingPlan, &'static str> {
    let group_sizes = groups
        .iter()
        .map(|group| {
            (
                (group.source_scope.clone(), group.token.clone()),
                group.document_count,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let matching_group_count = scores.len();
    let budget = groups.len().div_ceil(10).clamp(4, MAX_SELECTED_GROUPS);
    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .score
            .total_cmp(&left.1.score)
            .then_with(|| right.1.matched_terms.cmp(&left.1.matched_terms))
            .then_with(|| left.0.cmp(&right.0))
    });
    if ranked.len() <= budget {
        return Err("no_candidate_reduction");
    }
    if !coarse_boundary_is_separated(&ranked, budget) {
        return Err("coarse_score_margin");
    }
    ranked.truncate(budget);
    let selected_document_count = ranked
        .iter()
        .filter_map(|(key, _)| group_sizes.get(key))
        .sum::<usize>();
    if selected_document_count == 0
        || selected_document_count.saturating_mul(100)
            > population.saturating_mul(MAX_SELECTED_DOCUMENT_PERCENT)
    {
        return Err("candidate_budget");
    }
    let tokens = ranked
        .iter()
        .map(|((_, token), _)| token.as_str())
        .collect::<Vec<_>>();
    let selected_group_count = tokens.len();
    Ok(Bm25RoutingPlan {
        route_match: Some(format!("routing_key : ({})", tokens.join(" OR "))),
        explanation: Some(format!(
            "hierarchical_bm25 algorithm={ROUTING_ALGORITHM_VERSION} signal=aggregate_tf_idf selected_groups={selected_group_count}/{matching_group_count} selected_documents={selected_document_count}/{population} approximate=true"
        )),
    })
}

fn coarse_boundary_is_separated(ranked: &[((String, String), GroupScore)], budget: usize) -> bool {
    let (Some((_, selected)), Some((_, rejected))) =
        (ranked.get(budget.saturating_sub(1)), ranked.get(budget))
    else {
        return false;
    };
    if selected.score <= 0.0 {
        return false;
    }
    if selected.matched_terms > rejected.matched_terms {
        return true;
    }
    selected.score * 100.0 >= rejected.score * (100.0 + MIN_COARSE_SCORE_MARGIN_PERCENT)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
