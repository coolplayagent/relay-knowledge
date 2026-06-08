use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::storage::{GraphSearchRequest, StorageError};

use super::bm25::RawBm25Row;
use super::{ScoredHit, label_trigrams, scored_bm25_hit, split_labels};

const MIN_LIKE_QUERY_LEN: usize = 2;
const MIN_FUZZY_QUERY_LEN: usize = 3;
const FUZZY_SHORT_QUERY_MAX_DISTANCE: usize = 1;
const FUZZY_LONG_QUERY_MAX_DISTANCE: usize = 2;
const FUZZY_SHORT_QUERY_LENGTH_THRESHOLD: usize = 4;
const FALLBACK_CANDIDATE_LIMIT: usize = 200;
const FUZZY_LABEL_CANDIDATE_LIMIT: usize = FALLBACK_CANDIDATE_LIMIT * 8;
const FUZZY_MATCHED_NAME_LIMIT: usize = FALLBACK_CANDIDATE_LIMIT;

const SELECT_COLUMNS: &str = "\
            graph_bm25.document_id,\n\
            graph_bm25.document_kind,\n\
            graph_bm25.evidence_id,\n\
            graph_bm25.parent_evidence_id,\n\
            graph_bm25.modality,\n\
            graph_bm25.source_scope,\n\
            graph_bm25.source_path,\n\
            graph_bm25.entity_labels,\n\
            graph_bm25.content";

const JOIN_EVIDENCE: &str = "\
        FROM graph_bm25\n\
        LEFT JOIN evidence e\n\
          ON graph_bm25.document_kind = 'evidence'\n\
         AND e.id = graph_bm25.evidence_id";

fn scope_filter(scope_idx: u32, version_idx: u32) -> String {
    format!(
        "\
          AND (?{scope_idx} IS NULL OR graph_bm25.source_scope = ?{scope_idx})\n\
          AND graph_bm25.created_graph_version <= ?{version_idx}\n\
          AND (\n\
              graph_bm25.document_kind != 'evidence'\n\
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

pub(super) fn fallback_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let query = request.query.trim();
    if query.len() < MIN_LIKE_QUERY_LEN {
        return Ok(Vec::new());
    }

    let exact_rows = exact_name_rows(connection, request)?;
    let like_rows = if exact_rows.is_empty() && query.len() >= MIN_LIKE_QUERY_LEN {
        like_substring_rows(connection, request)?
    } else {
        Vec::new()
    };
    let fuzzy_rows =
        if exact_rows.is_empty() && like_rows.is_empty() && query.len() >= MIN_FUZZY_QUERY_LEN {
            fuzzy_levenshtein_rows(connection, request)?
        } else {
            Vec::new()
        };

    let all_candidates = merge_fallback_candidates(exact_rows, like_rows, fuzzy_rows);
    convert_fallback_candidates(connection, request, all_candidates)
}

fn exact_name_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let name_exact = request.query.trim().to_ascii_lowercase();
    let name_like = json_string_contains_like_pattern(&name_exact)?;
    let limit = FALLBACK_CANDIDATE_LIMIT.min(request.limit);
    let filter = scope_filter(3, 4);
    let sql = format!(
        "\
        SELECT\n\
            {SELECT_COLUMNS}\n\
        {JOIN_EVIDENCE}\n\
        WHERE (\n\
            graph_bm25.entity_labels LIKE ?1 ESCAPE '\\'\n\
            OR LOWER(graph_bm25.content) = ?2\n\
        )\n\
        {filter}\n\
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
    let limit = FALLBACK_CANDIDATE_LIMIT.min(request.limit);
    let filter = scope_filter(2, 3);
    let sql = format!(
        "\
        SELECT\n\
            {SELECT_COLUMNS}\n\
        {JOIN_EVIDENCE}\n\
        WHERE (\n\
            graph_bm25.content LIKE ?1 ESCAPE '\\'\n\
            OR graph_bm25.source_path LIKE ?1 ESCAPE '\\'\n\
        )\n\
        {filter}\n\
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

fn fuzzy_levenshtein_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let query = request.query.trim();
    let max_distance = adaptive_max_distance(query);
    let limit = FALLBACK_CANDIDATE_LIMIT.min(request.limit);

    let distinct_names = label_trigrams::fuzzy_label_candidates(
        connection,
        request,
        query,
        max_distance,
        FUZZY_LABEL_CANDIDATE_LIMIT,
    )?;
    let matching_names = matching_fuzzy_names(distinct_names, query, max_distance);

    if matching_names.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidates =
        fuzzy_rows_for_names(connection, request, &matching_names, max_distance, limit)?;
    sort_fuzzy_candidates(&mut candidates);
    Ok(candidates)
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
    let filter = scope_filter(scope_idx as u32, version_idx as u32);

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
            WHERE (?{scope_idx} IS NULL OR grams.source_scope = ?{scope_idx})\n\
              AND grams.created_graph_version <= ?{version_idx}\n\
            GROUP BY grams.document_id\n\
            ORDER BY match_score DESC,\n\
                     rank_order ASC,\n\
                     grams.document_id ASC\n\
            LIMIT ?{limit_idx}\n\
        )\n\
        SELECT\n\
            {SELECT_COLUMNS},\n\
            candidate_docs.match_score\n\
        FROM candidate_docs\n\
        JOIN graph_bm25\n\
          ON graph_bm25.document_id = candidate_docs.document_id\n\
        LEFT JOIN evidence e\n\
          ON graph_bm25.document_kind = 'evidence'\n\
         AND e.id = graph_bm25.evidence_id\n\
        WHERE 1 = 1\n\
        {filter}\n\
        ORDER BY candidate_docs.match_score DESC,\n\
                 candidate_docs.rank_order ASC,\n\
                 graph_bm25.document_id ASC"
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
            let name_lower = name.to_ascii_lowercase();
            let distance = levenshtein_distance(&query_lower, &name_lower);
            (distance <= max_distance).then_some(FuzzyNameMatch {
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

#[allow(clippy::needless_range_loop)]
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev = vec![0usize; b_len + 1];
    let mut curr = vec![0usize; b_len + 1];

    for j in 0..=b_len {
        prev[j] = j;
    }

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

fn merge_fallback_candidates(
    exact: Vec<FallbackCandidate>,
    like: Vec<FallbackCandidate>,
    fuzzy: Vec<FallbackCandidate>,
) -> Vec<FallbackCandidate> {
    let mut seen = std::collections::BTreeSet::new();
    let mut merged = Vec::new();

    for candidate in exact.into_iter().chain(like).chain(fuzzy) {
        if seen.insert(candidate.document_id.clone()) {
            merged.push(candidate);
        }
    }

    merged
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
            };
            let mut scored =
                scored_bm25_hit(connection, row, request.graph_version, &facts_by_evidence)?;
            scored.source_score = candidate.match_score;
            Ok(scored)
        })
        .collect()
}

#[cfg(test)]
#[path = "bm25_fallback_tests.rs"]
mod tests;
