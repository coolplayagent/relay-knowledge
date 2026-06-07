use rusqlite::{Connection, params};

use crate::storage::{GraphSearchRequest, StorageError};

use super::bm25::RawBm25Row;
use super::{ScoredHit, scored_bm25_hit, split_labels};

const MIN_LIKE_QUERY_LEN: usize = 2;
const MIN_FUZZY_QUERY_LEN: usize = 3;
const FUZZY_SHORT_QUERY_MAX_DISTANCE: usize = 1;
const FUZZY_LONG_QUERY_MAX_DISTANCE: usize = 2;
const FUZZY_SHORT_QUERY_LENGTH_THRESHOLD: usize = 4;
const FALLBACK_CANDIDATE_LIMIT: usize = 200;

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
    let name_like = format!(
        "%\"{}\"%",
        name_exact.replace('%', "\\%").replace('_', "\\_")
    );
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
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn like_substring_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let query_like = format!(
        "%{}%",
        request.query.trim().replace('%', "\\%").replace('_', "\\_")
    );
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
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn fuzzy_levenshtein_rows(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<FallbackCandidate>, StorageError> {
    let query = request.query.trim();
    let max_distance = adaptive_max_distance(query);
    let limit = FALLBACK_CANDIDATE_LIMIT.min(request.limit);

    let distinct_names = collect_distinct_symbol_names(connection, request)?;
    let matching_names: Vec<String> = distinct_names
        .into_iter()
        .filter(|name| {
            let name_lower = name.to_ascii_lowercase();
            let query_lower = query.to_ascii_lowercase();
            levenshtein_distance(&query_lower, &name_lower) <= max_distance
        })
        .collect();

    if matching_names.is_empty() {
        return Ok(Vec::new());
    }

    let n = matching_names.len();
    let mut like_clauses = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i + 1;
        like_clauses.push(format!("graph_bm25.content LIKE ?{idx} ESCAPE '\\'"));
    }
    let like_expr = like_clauses.join(" OR ");
    let scope_idx = (n + 1) as u32;
    let version_idx = (n + 2) as u32;
    let limit_idx = (n + 3) as u32;
    let filter = scope_filter(scope_idx, version_idx);

    let sql = format!(
        "\
        SELECT\n\
            {SELECT_COLUMNS}\n\
        {JOIN_EVIDENCE}\n\
        WHERE ({like_expr})\n\
        {filter}\n\
        LIMIT ?{limit_idx}"
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for name in &matching_names {
        let name_like = format!("%{}%", name.replace('%', "\\%").replace('_', "\\_"));
        param_values.push(Box::new(name_like));
    }
    param_values.push(Box::new(request.source_scope.clone()));
    param_values.push(Box::new(request.graph_version.get()));
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(param_refs.as_slice(), |row| {
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
            match_score: 0.25,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn collect_distinct_symbol_names(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "\
        SELECT DISTINCT entity_labels\n\
        FROM graph_bm25\n\
        WHERE (?1 IS NULL OR source_scope = ?1)\n\
          AND created_graph_version <= ?2\n\
          AND document_kind IN ('code_symbol', 'code_chunk')\n\
        LIMIT ?3",
    )?;
    let limit = FALLBACK_CANDIDATE_LIMIT * 5;
    let rows = statement.query_map(
        params![
            request.source_scope.as_deref(),
            request.graph_version.get(),
            limit
        ],
        |row| {
            let labels_str: String = row.get(0)?;
            Ok(labels_str)
        },
    )?;

    let mut names = Vec::new();
    for row in rows {
        let labels_json = row.map_err(StorageError::from)?;
        if let Ok(labels) = serde_json::from_str::<Vec<String>>(&labels_json) {
            for label in labels {
                if !label.is_empty() && label.len() >= 2 {
                    names.push(label);
                }
            }
        }
    }
    names.sort();
    names.dedup();

    Ok(names)
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
