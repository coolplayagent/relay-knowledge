//! Bounded exact, substring, and fuzzy fallback retrieval when FTS is unavailable.

use std::ops::Deref;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

use crate::storage::{GraphSearchRequest, StorageError};

use super::bm25::RawBm25Row;
use super::{ScoredHit, label_trigrams, scored_bm25_hit, split_labels};

const MIN_LIKE_QUERY_LEN: usize = 2;
const MIN_FUZZY_QUERY_LEN: usize = 3;
const FUZZY_SHORT_QUERY_MAX_DISTANCE: usize = 1;
const FUZZY_LONG_QUERY_MAX_DISTANCE: usize = 2;
const FUZZY_SHORT_QUERY_LENGTH_THRESHOLD: usize = 4;
const FALLBACK_CANDIDATE_LIMIT: usize = 1_000;
const FUZZY_LABEL_CANDIDATE_LIMIT: usize = FALLBACK_CANDIDATE_LIMIT * 8;
const FUZZY_MATCHED_NAME_LIMIT: usize = FALLBACK_CANDIDATE_LIMIT;
const MAX_FUZZY_QUERY_BYTES: usize = 128;
const MAX_FLAT_FALLBACK_SCAN_DOCUMENTS: usize = 4_096;

const SELECT_COLUMNS: &str = "\
            fallback.document_id,\n\
            fallback.document_kind,\n\
            fallback.evidence_id,\n\
            fallback.parent_evidence_id,\n\
            fallback.modality,\n\
            fallback.source_scope,\n\
            fallback.source_path,\n\
            fallback.entity_labels_json,\n\
            fallback.content";

const JOIN_EVIDENCE: &str = "\
        FROM graph_semantic_documents fallback\n\
        LEFT JOIN evidence e\n\
          ON fallback.document_kind = 'evidence'\n\
         AND e.id = fallback.evidence_id";

fn scope_filter(source_scope: Option<&str>, scope_idx: u32, version_idx: u32) -> String {
    let source_scope_filter = source_scope.map_or_else(
        || "1 = 1".to_owned(),
        |_| format!("fallback.source_scope = ?{scope_idx}"),
    );
    format!(
        "\
          AND {source_scope_filter}\n\
          AND fallback.created_graph_version <= ?{version_idx}\n\
          AND (\n\
              fallback.document_kind != 'evidence'\n\
              OR e.status IN ('accepted', 'proposed')\n\
          )"
    )
}

struct FallbackCandidate {
    document_id: String,
    document_kind: String,
    evidence_id: String,
    parent_evidence_id: Option<String>,
    modality: String,
    source_scope: String,
    source_path: Option<String>,
    entity_labels: Vec<String>,
    content: String,
    match_score: f64,
}

pub(super) struct FallbackCandidates {
    pub(super) hits: Vec<ScoredHit>,
    pub(super) degraded_reason: Option<String>,
}

impl Deref for FallbackCandidates {
    type Target = [ScoredHit];

    fn deref(&self) -> &Self::Target {
        &self.hits
    }
}

pub(super) fn fallback_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<FallbackCandidates, StorageError> {
    let query = request.query.trim();
    if query.len() < MIN_LIKE_QUERY_LEN {
        return Ok(FallbackCandidates {
            hits: Vec::new(),
            degraded_reason: None,
        });
    }

    let flat_scan_allowed = flat_fallback_scan_is_bounded(connection, request)?;
    let exact_rows = if flat_scan_allowed {
        exact_name_rows(connection, request)?
    } else {
        Vec::new()
    };
    let like_rows = if flat_scan_allowed
        && distinct_fallback_candidate_count(exact_rows.iter()) < request.limit
        && query.len() >= MIN_LIKE_QUERY_LEN
    {
        like_substring_rows(connection, request)?
    } else {
        Vec::new()
    };
    let fuzzy_attempted = distinct_fallback_candidate_count(exact_rows.iter().chain(&like_rows))
        < request.limit
        && query.len() >= MIN_FUZZY_QUERY_LEN
        && query.len() <= MAX_FUZZY_QUERY_BYTES;
    let fuzzy_outcome = if fuzzy_attempted {
        fuzzy_levenshtein_rows(connection, request)?
    } else {
        FuzzyRows::default()
    };

    let all_candidates = merge_fallback_candidates(exact_rows, like_rows, fuzzy_outcome.rows);
    let mut degraded_reasons = Vec::new();
    if !flat_scan_allowed {
        degraded_reasons.push(format!(
            "bm25 exact/substring fallback skipped because the authorized candidate corpus exceeds {MAX_FLAT_FALLBACK_SCAN_DOCUMENTS} documents"
        ));
    }
    if fuzzy_attempted {
        if let Some(reason) = label_gram_degraded_reason(connection, request)? {
            degraded_reasons.push(reason);
        }
    }
    if fuzzy_outcome.posting_budget_exhausted {
        degraded_reasons.push(
            "label fuzzy fallback skipped because its posting budget was exhausted".to_owned(),
        );
    }
    Ok(FallbackCandidates {
        hits: convert_fallback_candidates(connection, request, all_candidates)?,
        degraded_reason: (!degraded_reasons.is_empty()).then(|| degraded_reasons.join("; ")),
    })
}

fn label_gram_degraded_reason(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Option<String>, StorageError> {
    const DEGRADED_STATES: [&str; 5] = [
        "pending",
        "not_refreshed",
        "skipped:label_count",
        "skipped:label_utf8_bytes",
        "skipped:gram_count",
    ];
    let graph_version = i64::try_from(request.graph_version.get()).map_err(|_| {
        StorageError::InvalidInput("graph version is too large for sqlite".to_owned())
    })?;
    let state = if let Some(source_scope) = request.source_scope.as_deref() {
        connection
            .query_row(
                "SELECT label_gram_state
                 FROM graph_bm25_route_documents
                 WHERE label_gram_state IN (?1, ?2, ?3, ?4, ?5)
                   AND source_scope = ?6
                   AND created_graph_version <= ?7
                 LIMIT 1",
                params![
                    DEGRADED_STATES[0],
                    DEGRADED_STATES[1],
                    DEGRADED_STATES[2],
                    DEGRADED_STATES[3],
                    DEGRADED_STATES[4],
                    source_scope,
                    graph_version,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "SELECT label_gram_state
                 FROM graph_bm25_route_documents
                 WHERE label_gram_state IN (?1, ?2, ?3, ?4, ?5)
                   AND created_graph_version <= ?6
                 LIMIT 1",
                params![
                    DEGRADED_STATES[0],
                    DEGRADED_STATES[1],
                    DEGRADED_STATES[2],
                    DEGRADED_STATES[3],
                    DEGRADED_STATES[4],
                    graph_version,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    };
    Ok(state.map(|state| format!("label fuzzy fallback degraded: {state}")))
}

fn flat_fallback_scan_is_bounded(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<bool, StorageError> {
    let probe_limit = MAX_FLAT_FALLBACK_SCAN_DOCUMENTS.saturating_add(1);
    let graph_version = i64::try_from(request.graph_version.get()).map_err(|_| {
        StorageError::InvalidInput("graph version is too large for sqlite".to_owned())
    })?;
    let observed = if let Some(source_scope) = request.source_scope.as_deref() {
        connection.query_row(
            "SELECT COUNT(*) FROM (
                 SELECT document_id
                 FROM graph_semantic_documents
                 WHERE source_scope = ?1
                   AND created_graph_version <= ?2
                 LIMIT ?3
             )",
            params![source_scope, graph_version, probe_limit],
            |row| row.get::<_, usize>(0),
        )?
    } else {
        connection.query_row(
            "SELECT COUNT(*) FROM (
                 SELECT document_id
                 FROM graph_semantic_documents
                 WHERE created_graph_version <= ?1
                 LIMIT ?2
             )",
            params![graph_version, probe_limit],
            |row| row.get::<_, usize>(0),
        )?
    };
    Ok(observed <= MAX_FLAT_FALLBACK_SCAN_DOCUMENTS)
}

fn exact_name_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let name_exact = request.query.trim().to_ascii_lowercase();
    let name_like = json_string_contains_like_pattern(&name_exact)?;
    let limit = request.limit.min(FALLBACK_CANDIDATE_LIMIT);
    let filter = scope_filter(request.source_scope.as_deref(), 3, 4);
    let sql = format!(
        "\
        SELECT\n\
            {SELECT_COLUMNS}\n\
        {JOIN_EVIDENCE}\n\
        WHERE (\n\
            fallback.entity_labels_json LIKE ?1 ESCAPE '\\'\n\
            OR LOWER(fallback.content) = ?2\n\
        )\n\
        {filter}\n\
        GROUP BY fallback.document_kind = 'evidence', CASE\n\
            WHEN fallback.document_kind = 'evidence'\n\
                THEN COALESCE(fallback.parent_evidence_id, fallback.evidence_id)\n\
            ELSE fallback.document_id\n\
        END\n\
        ORDER BY fallback.document_id\n\
        LIMIT ?5"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            name_like,
            name_exact,
            request.source_scope.as_deref(),
            request.graph_version.get(),
            limit
        ],
        |row| {
            Ok(FallbackCandidate {
                document_id: row.get(0)?,
                document_kind: row.get(1)?,
                evidence_id: row.get(2)?,
                parent_evidence_id: row.get(3)?,
                modality: row.get(4)?,
                source_scope: row.get(5)?,
                source_path: row.get(6)?,
                entity_labels: split_labels(row.get(7)?),
                content: row.get(8)?,
                match_score: 1.0,
            })
        },
    )?;
    let mut candidates = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    sort_fallback_candidates(&mut candidates);
    Ok(candidates)
}

fn like_substring_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let query_like = contains_like_pattern(request.query.trim());
    let limit = request.limit.min(FALLBACK_CANDIDATE_LIMIT);
    let filter = scope_filter(request.source_scope.as_deref(), 2, 3);
    let sql = format!(
        "\
        SELECT\n\
            {SELECT_COLUMNS}\n\
        {JOIN_EVIDENCE}\n\
        WHERE (\n\
            fallback.content LIKE ?1 ESCAPE '\\'\n\
            OR fallback.source_path LIKE ?1 ESCAPE '\\'\n\
        )\n\
        {filter}\n\
        GROUP BY fallback.document_kind = 'evidence', CASE\n\
            WHEN fallback.document_kind = 'evidence'\n\
                THEN COALESCE(fallback.parent_evidence_id, fallback.evidence_id)\n\
            ELSE fallback.document_id\n\
        END\n\
        ORDER BY fallback.document_id\n\
        LIMIT ?4"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            query_like,
            request.source_scope.as_deref(),
            request.graph_version.get(),
            limit
        ],
        |row| {
            Ok(FallbackCandidate {
                document_id: row.get(0)?,
                document_kind: row.get(1)?,
                evidence_id: row.get(2)?,
                parent_evidence_id: row.get(3)?,
                modality: row.get(4)?,
                source_scope: row.get(5)?,
                source_path: row.get(6)?,
                entity_labels: split_labels(row.get(7)?),
                content: row.get(8)?,
                match_score: 0.5,
            })
        },
    )?;
    let mut candidates = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    sort_fallback_candidates(&mut candidates);
    Ok(candidates)
}

#[derive(Default)]
struct FuzzyRows {
    rows: Vec<FallbackCandidate>,
    posting_budget_exhausted: bool,
}

fn fuzzy_levenshtein_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<FuzzyRows, StorageError> {
    let query = request.query.trim();
    let max_distance = adaptive_max_distance(query);
    let limit = request.limit.min(FALLBACK_CANDIDATE_LIMIT);

    let label_candidates = label_trigrams::fuzzy_label_candidates(
        connection,
        request,
        query,
        max_distance,
        limit.saturating_mul(8).min(FUZZY_LABEL_CANDIDATE_LIMIT),
    )?;
    let matching_names = matching_fuzzy_names(label_candidates.names, query, max_distance);

    if matching_names.is_empty() {
        return Ok(FuzzyRows {
            rows: Vec::new(),
            posting_budget_exhausted: label_candidates.posting_budget_exhausted,
        });
    }

    let mut candidates =
        fuzzy_rows_for_names(connection, request, &matching_names, max_distance, limit)?;
    sort_fuzzy_candidates(&mut candidates);
    Ok(FuzzyRows {
        rows: candidates,
        posting_budget_exhausted: label_candidates.posting_budget_exhausted,
    })
}

fn fuzzy_rows_for_names(
    connection: &Connection,
    request: &GraphSearchRequest,
    name_matches: &[FuzzyNameMatch],
    max_distance: usize,
    limit: usize,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    if name_matches.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let match_rows = name_matches
        .iter()
        .map(|_| "(?, ?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let scope_idx = (name_matches.len() * 3) + 1;
    let version_idx = scope_idx + 1;
    let limit_idx = version_idx + 1;
    let filter = scope_filter(
        request.source_scope.as_deref(),
        scope_idx as u32,
        version_idx as u32,
    );
    let grams_scope_filter = request.source_scope.as_ref().map_or_else(
        || "1 = 1".to_owned(),
        |_| format!("grams.source_scope = ?{scope_idx}"),
    );

    let sql = format!(
        "\
        WITH matched_names(name_lower, match_score, rank_order) AS (VALUES {match_rows}),\n\
        candidate_docs AS (\n\
            SELECT grams.document_id,\n\
                   MAX(matched_names.match_score) AS match_score,\n\
                   MIN(matched_names.rank_order) AS rank_order\n\
            FROM graph_bm25_label_grams grams\n\
            JOIN matched_names\n\
              ON grams.label_lower = matched_names.name_lower\n\
            WHERE {grams_scope_filter}\n\
              AND grams.created_graph_version <= ?{version_idx}\n\
            GROUP BY grams.document_id\n\
        )\n\
        SELECT\n\
            {SELECT_COLUMNS},\n\
            candidate_docs.match_score\n\
        FROM candidate_docs\n\
        JOIN graph_semantic_documents fallback\n\
          ON fallback.document_id = candidate_docs.document_id\n\
        LEFT JOIN evidence e\n\
          ON fallback.document_kind = 'evidence'\n\
         AND e.id = fallback.evidence_id\n\
        WHERE 1 = 1\n\
        {filter}\n\
        GROUP BY fallback.document_kind = 'evidence', CASE\n\
            WHEN fallback.document_kind = 'evidence'\n\
                THEN COALESCE(fallback.parent_evidence_id, fallback.evidence_id)\n\
            ELSE fallback.document_id\n\
        END\n\
        ORDER BY MAX(candidate_docs.match_score) DESC,\n\
                 candidate_docs.rank_order ASC,\n\
                 fallback.document_id ASC\n\
        LIMIT ?{limit_idx}"
    );

    let mut values = Vec::with_capacity((name_matches.len() * 3) + 3);
    for (rank_order, name_match) in name_matches.iter().enumerate() {
        values.push(Value::Text(name_match.name_lower.clone()));
        values.push(Value::Real(fuzzy_match_score(
            name_match.distance,
            max_distance,
        )));
        values.push(Value::Integer(rank_order as i64));
    }
    let scope_value = request
        .source_scope
        .as_ref()
        .map_or(Value::Null, |scope| Value::Text(scope.clone()));
    values.push(scope_value);
    values.push(i64_value(request.graph_version.get(), "graph version")?);
    values.push(Value::Integer(limit as i64));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(FallbackCandidate {
            document_id: row.get(0)?,
            document_kind: row.get(1)?,
            evidence_id: row.get(2)?,
            parent_evidence_id: row.get(3)?,
            modality: row.get(4)?,
            source_scope: row.get(5)?,
            source_path: row.get(6)?,
            entity_labels: split_labels(row.get(7)?),
            content: row.get(8)?,
            match_score: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn i64_value(value: u64, name: &str) -> Result<Value, StorageError> {
    let converted = i64::try_from(value)
        .map_err(|_| StorageError::InvalidInput(format!("{name} is too large for sqlite")))?;
    Ok(Value::Integer(converted))
}

fn sort_fuzzy_candidates(candidates: &mut [FallbackCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .match_score
            .total_cmp(&left.match_score)
            .then_with(|| left.document_id.cmp(&right.document_id))
    });
}

fn sort_fallback_candidates(candidates: &mut [FallbackCandidate]) {
    candidates.sort_by(|left, right| left.document_id.cmp(&right.document_id));
}

fn fuzzy_match_score(distance: usize, max_distance: usize) -> f64 {
    0.25 + (max_distance.saturating_sub(distance) as f64 * 0.01)
}

struct FuzzyNameMatch {
    name: String,
    name_lower: String,
    distance: usize,
}

fn matching_fuzzy_names(
    distinct_names: Vec<String>,
    query: &str,
    max_distance: usize,
) -> Vec<FuzzyNameMatch> {
    let query_lower = query.to_ascii_lowercase();
    let mut matching_names = distinct_names
        .into_iter()
        .filter_map(|name| {
            if name.len() > MAX_FUZZY_QUERY_BYTES {
                return None;
            }
            let name_lower = name.to_ascii_lowercase();
            let distance = bounded_levenshtein_distance(&query_lower, &name_lower, max_distance)?;
            Some(FuzzyNameMatch {
                name,
                name_lower,
                distance,
            })
        })
        .collect::<Vec<_>>();
    matching_names.sort_by(|left, right| {
        left.distance
            .cmp(&right.distance)
            .then_with(|| left.name.cmp(&right.name))
    });
    matching_names.truncate(FUZZY_MATCHED_NAME_LIMIT);
    matching_names
}

pub(super) fn adaptive_max_distance(query: &str) -> usize {
    if query.len() <= FUZZY_SHORT_QUERY_LENGTH_THRESHOLD {
        FUZZY_SHORT_QUERY_MAX_DISTANCE
    } else {
        FUZZY_LONG_QUERY_MAX_DISTANCE
    }
}

fn bounded_levenshtein_distance(a: &str, b: &str, max_distance: usize) -> Option<usize> {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len.abs_diff(b_len) > max_distance {
        return None;
    }
    if a_len == 0 {
        return (b_len <= max_distance).then_some(b_len);
    }
    if b_len == 0 {
        return (a_len <= max_distance).then_some(a_len);
    }

    let outside_band = max_distance.saturating_add(1);
    let mut previous = vec![outside_band; b_len + 1];
    let mut current = vec![outside_band; b_len + 1];
    for (column, value) in previous
        .iter_mut()
        .enumerate()
        .take(max_distance.min(b_len) + 1)
    {
        *value = column;
    }

    for row in 1..=a_len {
        current.fill(outside_band);
        if row <= max_distance {
            current[0] = row;
        }
        let first_column = row.saturating_sub(max_distance).max(1);
        let last_column = row.saturating_add(max_distance).min(b_len);
        let mut row_minimum = outside_band;
        for column in first_column..=last_column {
            let substitution_cost = if a_chars[row - 1] == b_chars[column - 1] {
                0
            } else {
                1
            };
            current[column] = previous[column]
                .saturating_add(1)
                .min(current[column - 1].saturating_add(1))
                .min(previous[column - 1].saturating_add(substitution_cost))
                .min(outside_band);
            row_minimum = row_minimum.min(current[column]);
        }
        if row_minimum > max_distance {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }

    (previous[b_len] <= max_distance).then_some(previous[b_len])
}

fn merge_fallback_candidates(
    exact: Vec<FallbackCandidate>,
    like: Vec<FallbackCandidate>,
    fuzzy: Vec<FallbackCandidate>,
) -> Vec<FallbackCandidate> {
    let mut seen = std::collections::BTreeSet::new();
    let mut merged = Vec::new();

    for candidate in exact.into_iter().chain(like).chain(fuzzy) {
        if seen.insert(fallback_candidate_key(&candidate)) {
            merged.push(candidate);
        }
    }

    merged
}

fn distinct_fallback_candidate_count<'a>(
    candidates: impl IntoIterator<Item = &'a FallbackCandidate>,
) -> usize {
    candidates
        .into_iter()
        .map(fallback_candidate_key)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn fallback_candidate_key(candidate: &FallbackCandidate) -> String {
    if candidate.document_kind == "evidence" {
        format!(
            "evidence_group:{}",
            candidate
                .parent_evidence_id
                .as_deref()
                .unwrap_or(&candidate.evidence_id)
        )
    } else {
        format!("document:{}", candidate.document_id)
    }
}

fn json_string_contains_like_pattern(value: &str) -> Result<String, StorageError> {
    let json_string = serde_json::to_string(value)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    Ok(contains_like_pattern(&json_string))
}

fn contains_like_pattern(value: &str) -> String {
    format!("%{}%", escape_like_pattern(value))
}

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn convert_fallback_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
    candidates: Vec<FallbackCandidate>,
) -> Result<Vec<ScoredHit>, StorageError> {
    let evidence_ids: Vec<String> = candidates
        .iter()
        .filter(|c| c.document_kind == "evidence")
        .map(|c| c.evidence_id.clone())
        .collect();

    let facts_by_evidence = if evidence_ids.is_empty() {
        std::collections::BTreeMap::new()
    } else {
        super::context::facts_for_evidence_ids(connection, evidence_ids, request.graph_version)?
    };

    candidates
        .into_iter()
        .map(|candidate| {
            let row = RawBm25Row {
                document_id: candidate.document_id,
                document_kind: candidate.document_kind,
                evidence_id: candidate.evidence_id,
                parent_evidence_id: candidate.parent_evidence_id,
                modality: candidate.modality,
                source_scope: candidate.source_scope,
                source_path: candidate.source_path,
                entity_labels: candidate.entity_labels,
                content: candidate.content,
                rank: 0.0,
                explanation: None,
            };
            let mut scored =
                scored_bm25_hit(connection, row, request.graph_version, &facts_by_evidence)?;
            scored.source_score = candidate.match_score;
            Ok(scored)
        })
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
