//! Bounded SQLite FTS candidate reads and transient-query retry policy.

use std::{collections::BTreeSet, thread, time::Duration};

use rusqlite::{Connection, Params, Statement, params};

use crate::storage::{GraphSearchRequest, StorageError};

const GRAPH_BM25_QUERY_RETRY_DELAYS_MS: [u64; 3] = [5, 15, 45];
const INITIAL_BM25_CANDIDATE_MULTIPLIER: usize = 4;
const MAX_BM25_CANDIDATE_MULTIPLIER: usize = 16;
const MAX_BM25_RAW_CANDIDATES: usize = 16_000;

const BM25_SQL: &str = "
    SELECT
        graph_bm25.rowid,
        graph_bm25.document_id,
        graph_bm25.document_kind,
        graph_bm25.evidence_id,
        graph_bm25.parent_evidence_id,
        graph_bm25.rank
    FROM graph_bm25
    LEFT JOIN evidence e
      ON graph_bm25.document_kind = 'evidence'
     AND e.id = graph_bm25.evidence_id
    WHERE graph_bm25 MATCH ?1
      AND graph_bm25.rank MATCH
          'bm25(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0)'
      AND (?2 IS NULL OR graph_bm25.source_scope = ?2)
      AND graph_bm25.created_graph_version <= ?3
      AND (graph_bm25.document_kind != 'evidence' OR e.status IN ('accepted', 'proposed'))
    ORDER BY graph_bm25.rank
    LIMIT ?4
    ";

const BM25_HYDRATE_SQL: &str = "
    WITH selected(rowid, rank, ordinal) AS (
        SELECT CAST(json_extract(value, '$[0]') AS INTEGER),
               CAST(json_extract(value, '$[1]') AS REAL),
               CAST(key AS INTEGER)
        FROM json_each(?1)
    )
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
        selected.rank
    FROM selected
    JOIN graph_bm25 ON graph_bm25.rowid = selected.rowid
    ORDER BY selected.ordinal
    ";

pub(super) struct RawBm25Row {
    pub(super) document_id: String,
    pub(super) document_kind: String,
    pub(super) evidence_id: String,
    pub(super) parent_evidence_id: Option<String>,
    pub(super) modality: String,
    pub(super) source_scope: String,
    pub(super) source_path: Option<String>,
    pub(super) entity_labels: Vec<String>,
    pub(super) content: String,
    pub(super) rank: f64,
    pub(super) explanation: Option<String>,
}

struct RankedBm25Identity {
    rowid: i64,
    document_id: String,
    document_kind: String,
    evidence_id: String,
    parent_evidence_id: Option<String>,
    rank: f64,
}

struct RankedBm25Window {
    rows: Vec<RankedBm25Identity>,
    exhausted: bool,
}

pub(super) fn bm25_candidate_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
) -> Result<Vec<RawBm25Row>, StorageError> {
    let plan = super::bm25_routing::plan_query(connection, request)?;
    let routed_rows = candidate_rows_for_plan(connection, request, match_query, &plan);
    if plan.route_match.is_some() {
        match routed_rows {
            Ok(window)
                if !window.rows.is_empty()
                    && distinct_ranked_candidate_count(&window.rows) >= request.limit =>
            {
                return hydrate_candidate_rows(connection, request, window, &plan);
            }
            Ok(_) => {
                let flat_plan =
                    super::bm25_routing::Bm25RoutingPlan::flat("routed_candidate_retry");
                let window = candidate_rows_for_plan(connection, request, match_query, &flat_plan)?;
                return hydrate_candidate_rows(connection, request, window, &flat_plan);
            }
            Err(error) => return Err(error),
        }
    }
    hydrate_candidate_rows(connection, request, routed_rows?, &plan)
}

fn candidate_rows_for_plan(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
    plan: &super::bm25_routing::Bm25RoutingPlan,
) -> Result<RankedBm25Window, StorageError> {
    let maximum = request
        .limit
        .saturating_mul(MAX_BM25_CANDIDATE_MULTIPLIER)
        .max(request.limit)
        .min(MAX_BM25_RAW_CANDIDATES);
    let mut candidate_limit = request
        .limit
        .saturating_mul(INITIAL_BM25_CANDIDATE_MULTIPLIER)
        .max(request.limit)
        .min(maximum);
    loop {
        let rows =
            candidate_rows_with_retry(connection, request, match_query, plan, candidate_limit)?;
        let distinct_count = distinct_ranked_candidate_count(&rows);
        if distinct_count >= request.limit
            || rows.len() < candidate_limit
            || candidate_limit >= maximum
        {
            return Ok(RankedBm25Window {
                exhausted: rows.len() == candidate_limit
                    && distinct_count < request.limit
                    && candidate_limit >= maximum,
                rows,
            });
        }
        candidate_limit = candidate_limit.saturating_mul(2).min(maximum);
    }
}

#[cfg(test)]
fn distinct_candidate_count(rows: &[RawBm25Row]) -> usize {
    rows.iter()
        .map(|row| {
            if row.document_kind == "evidence" {
                (
                    true,
                    row.parent_evidence_id
                        .as_deref()
                        .unwrap_or(&row.evidence_id),
                )
            } else {
                (false, row.document_id.as_str())
            }
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn distinct_ranked_candidate_count(rows: &[RankedBm25Identity]) -> usize {
    rows.iter()
        .map(ranked_candidate_key)
        .collect::<BTreeSet<_>>()
        .len()
}

fn ranked_candidate_key(row: &RankedBm25Identity) -> (bool, &str) {
    if row.document_kind == "evidence" {
        (
            true,
            row.parent_evidence_id
                .as_deref()
                .unwrap_or(&row.evidence_id),
        )
    } else {
        (false, row.document_id.as_str())
    }
}

fn candidate_rows_with_retry(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
    plan: &super::bm25_routing::Bm25RoutingPlan,
    candidate_limit: usize,
) -> Result<Vec<RankedBm25Identity>, StorageError> {
    for delay_ms in GRAPH_BM25_QUERY_RETRY_DELAYS_MS {
        match bm25_candidate_rows_once(connection, request, match_query, plan, candidate_limit) {
            Ok(rows) => return Ok(rows),
            Err(error) if graph_bm25_query_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }

    bm25_candidate_rows_once(connection, request, match_query, plan, candidate_limit)
}

fn bm25_candidate_rows_once(
    connection: &Connection,
    request: &GraphSearchRequest,
    match_query: &str,
    plan: &super::bm25_routing::Bm25RoutingPlan,
    candidate_limit: usize,
) -> Result<Vec<RankedBm25Identity>, StorageError> {
    let planned_match = planned_match_query(request, match_query, plan);
    let mut statement = connection.prepare(BM25_SQL)?;
    collect_ranked_candidate_rows(
        &mut statement,
        params![
            planned_match,
            request.source_scope.as_deref(),
            request.graph_version.get(),
            candidate_limit,
        ],
    )
}

fn planned_match_query(
    request: &GraphSearchRequest,
    match_query: &str,
    plan: &super::bm25_routing::Bm25RoutingPlan,
) -> String {
    let business_match = format!(
        "{{source_scope source_path entity_labels entity_aliases content}} : ({match_query})"
    );
    let mut planned_match = business_match;
    if let Some(source_scope) = request.source_scope.as_deref() {
        let scope_token = super::bm25_routing::scope_token(source_scope);
        planned_match = format!("({planned_match}) AND (routing_key : {scope_token})");
    }
    if let Some(route_match) = plan.route_match.as_deref() {
        planned_match = format!("({planned_match}) AND ({route_match})");
    }
    planned_match
}

fn collect_ranked_candidate_rows<P: Params>(
    statement: &mut Statement<'_>,
    parameters: P,
) -> Result<Vec<RankedBm25Identity>, StorageError> {
    let rows = statement.query_map(parameters, |row| {
        Ok(RankedBm25Identity {
            rowid: row.get(0)?,
            document_id: row.get(1)?,
            document_kind: row.get(2)?,
            evidence_id: row.get(3)?,
            parent_evidence_id: row.get(4)?,
            rank: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn hydrate_candidate_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
    window: RankedBm25Window,
    plan: &super::bm25_routing::Bm25RoutingPlan,
) -> Result<Vec<RawBm25Row>, StorageError> {
    let RankedBm25Window {
        mut rows,
        exhausted,
    } = window;
    rows.sort_by(|left, right| {
        left.rank
            .total_cmp(&right.rank)
            .then_with(|| ranked_result_evidence_id(left).cmp(ranked_result_evidence_id(right)))
            .then_with(|| left.document_id.cmp(&right.document_id))
    });
    let mut seen = BTreeSet::new();
    let mut selected = Vec::with_capacity(request.limit);
    for row in rows {
        let key = if row.document_kind == "evidence" {
            format!(
                "evidence_group:{}",
                row.parent_evidence_id
                    .as_deref()
                    .unwrap_or(&row.evidence_id)
            )
        } else {
            format!("document:{}", row.document_id)
        };
        if seen.insert(key) {
            selected.push((row.rowid, row.rank));
            if selected.len() == request.limit {
                break;
            }
        }
    }
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let selected_json = serde_json::to_string(&selected)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let explanation = match (plan.explanation.as_deref(), exhausted) {
        (Some(plan), true) => Some(format!("{plan} candidate_window_exhausted=true")),
        (Some(plan), false) => Some(plan.to_owned()),
        (None, true) => Some("bm25 candidate_window_exhausted=true".to_owned()),
        (None, false) => None,
    };
    let mut statement = connection.prepare(BM25_HYDRATE_SQL)?;
    let rows = statement.query_map(params![selected_json], |row| {
        Ok(RawBm25Row {
            document_id: row.get(0)?,
            document_kind: row.get(1)?,
            evidence_id: row.get(2)?,
            parent_evidence_id: row.get(3)?,
            modality: row.get(4)?,
            source_scope: row.get(5)?,
            source_path: row.get(6)?,
            entity_labels: super::split_labels(row.get(7)?),
            content: row.get(8)?,
            rank: row.get(9)?,
            explanation: explanation.clone(),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn ranked_result_evidence_id(row: &RankedBm25Identity) -> &str {
    row.parent_evidence_id
        .as_deref()
        .unwrap_or(&row.evidence_id)
}

pub(super) fn graph_bm25_query_error_is_retryable(error: &StorageError) -> bool {
    match error {
        StorageError::Sqlite(error) => {
            graph_bm25_query_error_message_is_retryable(&error.to_string())
        }
        _ => false,
    }
}

pub(super) fn graph_bm25_query_error_message_is_retryable(message: &str) -> bool {
    super::graph_bm25_transient_error_message(message)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod bm25_tests;
