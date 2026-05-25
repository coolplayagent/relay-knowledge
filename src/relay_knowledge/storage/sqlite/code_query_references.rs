use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts, code_query_excerpts::reference_excerpt, code_query_rows::ReferenceRow,
    code_query_support::*, code_search_plannable_outage_reason, dedupe_sort_truncate,
    hit_from_parts, mark_hits_degraded, prepare_code_search_statement, required_scope,
    selected_row,
};

const REFERENCE_ASSIGNMENT_USAGE_BONUS: f64 = 1.4;
const REFERENCE_INDIRECT_CALL_USAGE_BONUS: f64 = 1.8;
const REFERENCE_MEMBER_CALL_USAGE_BONUS: f64 = 1.2;

struct ReferenceIdentityRows {
    rows: Vec<ReferenceRow>,
    saturated: bool,
}

pub(super) fn search_references(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        let identity_rows = search_reference_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| {
                identity.matches_symbol(
                    &row.name,
                    "",
                    row.target_hint.as_deref().unwrap_or_default(),
                    row.target_canonical_symbol_id
                        .as_deref()
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        identity_hits = reference_rows_to_hits(status, request, rows);
        if reference_identity_hits_can_answer_without_fts(
            request,
            identity,
            identity_hits.len(),
            saturated,
        ) {
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    }

    let reference_fts_rows = match search_reference_fts_rows(connection, status, request) {
        Ok(rows) => rows,
        Err(error) => {
            let Some(reason) = code_search_plannable_outage_reason(request, &error) else {
                return Err(error);
            };
            if identity_hits.is_empty() {
                return Err(error);
            }
            mark_hits_degraded(&mut identity_hits, &reason);
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    };
    let mut hits = reference_rows_to_hits(status, request, reference_fts_rows);
    hits.extend(identity_hits);

    Ok(hits)
}

fn search_reference_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
) -> Result<ReferenceIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("r.path", status, request);
    let language_filter = language_filter_sql_for_column("f.language_id", status, request);
    let direct_limit = reference_identity_candidate_limit(request);
    let sql = reference_rows_sql(&format!(
        "
          AND r.name = ?
          {path_filter}
          {language_filter}
        "
    ));
    let mut values = vec![
        Value::Text(required_scope(status)?.to_owned()),
        Value::Text(identity.leaf_name().to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_reference)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(ReferenceIdentityRows { rows, saturated })
}

fn search_reference_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ReferenceRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = reference_rows_sql(&format!(
        "
          AND r.reference_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'reference'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        "
    ));
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Reference),
            candidate_limit(request, CandidateLayer::Reference),
        )),
        row_to_reference,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn reference_rows_sql(predicate_sql: &str) -> String {
    format!(
        "
        SELECT r.file_id, r.path, f.language_id, r.name, r.kind,
               r.target_symbol_snapshot_id, r.byte_start, r.byte_end,
               r.line_start, r.line_end, r.target_hint, r.resolution_state,
               r.confidence_basis_points, r.confidence_tier, s.canonical_symbol_id,
               (
                   SELECT chunk.content
                   FROM code_repository_chunks chunk
                   WHERE chunk.source_scope = r.source_scope
                     AND chunk.path = r.path
                     AND chunk.line_start <= r.line_start
                     AND chunk.line_end >= r.line_start
                   ORDER BY
                     (chunk.line_end - chunk.line_start) ASC,
                     chunk.line_start DESC,
                     chunk.chunk_id ASC
                   LIMIT 1
               ) AS source_excerpt,
               (
                   SELECT chunk.line_start
                   FROM code_repository_chunks chunk
                   WHERE chunk.source_scope = r.source_scope
                     AND chunk.path = r.path
                     AND chunk.line_start <= r.line_start
                     AND chunk.line_end >= r.line_start
                   ORDER BY
                     (chunk.line_end - chunk.line_start) ASC,
                     chunk.line_start DESC,
                     chunk.chunk_id ASC
                   LIMIT 1
               ) AS source_excerpt_line_start
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.source_scope = r.source_scope AND f.path = r.path
        LEFT JOIN code_repository_symbols s
            ON s.source_scope = r.source_scope
           AND s.symbol_snapshot_id = r.target_symbol_snapshot_id
        WHERE r.source_scope = ?
          {predicate_sql}
        ORDER BY r.path ASC, r.line_start ASC
        LIMIT ?
        "
    )
}

fn row_to_reference(row: &Row<'_>) -> rusqlite::Result<ReferenceRow> {
    Ok(ReferenceRow {
        file_id: row.get(0)?,
        path: row.get(1)?,
        language_id: row.get(2)?,
        name: row.get(3)?,
        kind: row.get(4)?,
        target_symbol_snapshot_id: row.get(5)?,
        byte_range: RepositoryCodeRange {
            start: row.get(6)?,
            end: row.get(7)?,
        },
        line_range: RepositoryCodeRange {
            start: row.get(8)?,
            end: row.get(9)?,
        },
        target_hint: row.get(10)?,
        resolution_state: row.get(11)?,
        confidence_basis_points: row.get(12)?,
        confidence_tier: row.get(13)?,
        target_canonical_symbol_id: row.get(14)?,
        source_excerpt: row.get(15)?,
        source_excerpt_line_start: row.get(16)?,
    })
}

fn reference_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<ReferenceRow>,
) -> Vec<CodeRetrievalHit> {
    let score_query = ScoreQuery::new(&request.query);

    rows.into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let base_score = score_query.score([
                row.name.as_str(),
                row.kind.as_str(),
                row.target_hint.as_deref().unwrap_or_default(),
                row.target_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ]) + scoped_identity_query_bonus(
                &request.query,
                [
                    row.target_hint.as_deref().unwrap_or_default(),
                    row.target_canonical_symbol_id
                        .as_deref()
                        .unwrap_or_default(),
                ],
            );
            let focused_source_excerpt = focused_reference_source_excerpt(&row);
            let score = base_score
                + reference_usage_context_bonus(
                    base_score,
                    &row.name,
                    focused_source_excerpt.as_deref(),
                    request,
                );
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range: row.line_range,
                        symbol_snapshot_id: row.target_symbol_snapshot_id,
                        canonical_symbol_id: row.target_canonical_symbol_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::Reference],
                        score: score + 1.5,
                        excerpt: reference_excerpt(
                            focused_source_excerpt.as_deref(),
                            &row.kind,
                            &row.name,
                        ),
                        degraded_reason: None,
                        edge_kind: Some(row.kind),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
                    },
                )
            })
        })
        .collect()
}

fn focused_reference_source_excerpt(row: &ReferenceRow) -> Option<String> {
    let source_excerpt = row.source_excerpt.as_deref()?;
    let Some(line_start) = row.source_excerpt_line_start else {
        return Some(source_excerpt.to_owned());
    };
    let offset = row.line_range.start.checked_sub(line_start)?;
    let line = source_excerpt
        .lines()
        .nth(usize::try_from(offset).ok()?)?
        .trim();
    if line.is_empty() || identifier_ranges(line, &row.name).next().is_none() {
        Some(source_excerpt.to_owned())
    } else {
        Some(line.to_owned())
    }
}

pub(super) fn reference_usage_context_bonus(
    base_score: f64,
    name: &str,
    source_excerpt: Option<&str>,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let Some(source_excerpt) = source_excerpt else {
        return 0.0;
    };

    source_excerpt
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('*'))
        .filter_map(|line| reference_line_usage_bonus(line, name))
        .fold(0.0, f64::max)
}

fn reference_line_usage_bonus(line: &str, name: &str) -> Option<f64> {
    identifier_ranges(line, name)
        .filter(|(start, end)| !line_declares_reference_name(line, name, *start, *end))
        .map(|(start, end)| reference_identifier_usage_bonus(line, start, end))
        .max_by(f64::total_cmp)
        .filter(|bonus| *bonus > 0.0)
}

fn reference_identifier_usage_bonus(line: &str, start: usize, end: usize) -> f64 {
    let before = line.get(..start).unwrap_or_default();
    let after = line.get(end..).unwrap_or_default().trim_start();
    if identifier_is_indirect_call(after) {
        return REFERENCE_INDIRECT_CALL_USAGE_BONUS;
    }
    if identifier_is_member_call(before, after) {
        return REFERENCE_MEMBER_CALL_USAGE_BONUS;
    }
    if identifier_is_assignment_value(before) {
        return REFERENCE_ASSIGNMENT_USAGE_BONUS;
    }

    0.0
}

fn line_declares_reference_name(line: &str, name: &str, start: usize, end: usize) -> bool {
    let before = line.get(..start).unwrap_or_default().trim_end();
    let after = line.get(end..).unwrap_or_default().trim_start();
    if before.ends_with('.') || before.ends_with("->") || identifier_is_assignment_value(before) {
        return false;
    }
    if after.starts_with('(') && declaration_prefix_before_name(before) {
        return true;
    }
    if after.starts_with('[') && array_declarator_has_initializer(after) {
        return true;
    }

    declaration_prefix_before_name(before) && before.split_whitespace().last() != Some(name)
}

fn declaration_prefix_before_name(before: &str) -> bool {
    let token_count = before.split_whitespace().count();
    token_count >= 1
        && before
            .chars()
            .all(|character| !matches!(character, '=' | '+' | '-' | '*' | '/' | '%' | '?'))
}

fn array_declarator_has_initializer(after: &str) -> bool {
    let Some(equals_index) = after.find('=') else {
        return false;
    };
    !after
        .get(..equals_index)
        .is_some_and(|prefix| prefix.contains(')'))
}

fn identifier_is_assignment_value(before: &str) -> bool {
    before
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| character == '=')
}

fn identifier_is_indirect_call(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('[') else {
        return false;
    };
    let Some((_, tail)) = rest.split_once(']') else {
        return false;
    };
    tail.trim_start().starts_with('(')
}

fn identifier_is_member_call(before: &str, after: &str) -> bool {
    after.starts_with('(')
        && (before.trim_end().ends_with('.') || before.trim_end().ends_with("->"))
}

fn identifier_ranges<'a>(
    line: &'a str,
    name: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    line.match_indices(name).filter_map(|(start, _)| {
        let end = start + name.len();
        let has_start_boundary = line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|c| !identifier_char(c))
        });
        let has_end_boundary = line
            .get(end..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !identifier_char(c)));
        (has_start_boundary && has_end_boundary).then_some((start, end))
    })
}

fn identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn reference_identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && request.code_query_kind == CodeQueryKind::References
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && specific_reference_identity_leaf(identity.leaf_name())))
}

fn reference_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Reference).min(200)
}

fn specific_reference_identity_leaf(leaf_name: &str) -> bool {
    leaf_name.len() >= 8 || leaf_name.contains('_') || has_case_boundary(leaf_name)
}

fn has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn reference_identity_fast_path_requires_specific_bounded_hits() {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        let request = CodeRetrievalRequest::new(
            "TargetThing",
            selector,
            CodeQueryKind::References,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let identity =
            SymbolIdentityQuery::from_query("TargetThing").expect("identity query should parse");

        assert!(reference_identity_hits_can_answer_without_fts(
            &request, &identity, 3, false
        ));
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request, &identity, 11, false
        ));
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request, &identity, 3, true
        ));
        let broad_identity =
            SymbolIdentityQuery::from_query("State").expect("identity query should parse");
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request,
            &broad_identity,
            1,
            false
        ));
    }
}
