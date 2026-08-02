use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts, code_search_plannable_outage_reason,
    excerpts::reference_excerpt,
    filter_dedupe_sort_truncate, has_query_field_hit_filters, hit_from_parts, mark_hits_degraded,
    prepare_code_search_statement, query_field_filtered_hits_for_gate,
    relevance::*,
    required_scope,
    rows::ReferenceRow,
    scoring::path_ranking::{
        query_mentions_test_or_benchmark, reference_source_path_bonus, reference_test_path_penalty,
    },
    selected_row,
};

mod call_shape;
mod identifier_text;
mod identity_gate;
mod same_name_path;
mod type_context;

use self::call_shape::{
    identifier_is_indirect_call, identifier_is_member_call, identifier_is_plain_call,
};
use self::identifier_text::identifier_ranges;
use self::identity_gate::{
    reference_identity_candidate_limit, reference_identity_hits_can_answer_without_fts,
};
use self::same_name_path::reference_same_name_file_penalty;
use self::type_context::{
    ParameterTypeContext, parameter_type_context, type_annotation_context_prefix,
    type_reference_usage_bonus,
};

const REFERENCE_ASSIGNMENT_USAGE_BONUS: f64 = 1.4;
const REFERENCE_INDIRECT_CALL_USAGE_BONUS: f64 = 1.8;
const REFERENCE_MEMBER_CALL_USAGE_BONUS: f64 = 1.2;
const REFERENCE_RETURN_USAGE_BONUS: f64 = 1.45;
const REFERENCE_PLAIN_CALL_USAGE_BONUS: f64 = 1.05;
const REFERENCE_RETURN_CALL_USAGE_BONUS: f64 = 1.55;
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
        let filtered_identity_hits = has_query_field_hit_filters(request)
            .then(|| query_field_filtered_hits_for_gate(&identity_hits, request));
        let identity_gate_hit_count = filtered_identity_hits
            .as_ref()
            .map_or(identity_hits.len(), Vec::len);
        if reference_identity_hits_can_answer_without_fts(
            request,
            identity,
            identity_gate_hit_count,
            saturated,
        ) {
            if let Some(mut hits) = filtered_identity_hits {
                hits.truncate(request.limit);
                return Ok(hits);
            }
            filter_dedupe_sort_truncate(&mut identity_hits, request);
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
            filter_dedupe_sort_truncate(&mut identity_hits, request);
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
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let direct_limit = reference_identity_candidate_limit(request);
    let sql = reference_rows_sql(&format!(
        "
          AND r.name = ?
          {path_filter}
          {language_filter}
          {generated_filter}
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
    push_language_filter_values(&mut values, &request.query_language_filters);
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
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = reference_rows_sql(&format!(
        "
          AND r.reference_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'reference'
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (SELECT 1 FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path AND fts_file.is_generated != 0))
              ORDER BY coalesce((SELECT fts_file.is_generated FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path LIMIT 1), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
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
               , f.is_generated
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.source_scope = r.source_scope AND f.path = r.path
        LEFT JOIN code_repository_symbols s
            ON s.source_scope = r.source_scope
           AND s.symbol_snapshot_id = r.target_symbol_snapshot_id
        WHERE r.source_scope = ?
          {predicate_sql}
        ORDER BY f.is_generated ASC, r.path ASC, r.line_start ASC
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
        is_generated: row.get::<_, i64>(17)? != 0,
    })
}

fn reference_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<ReferenceRow>,
) -> Vec<CodeRetrievalHit> {
    let score_query = ScoreQuery::new(&request.query);
    let query_has_test_intent = query_mentions_test_or_benchmark(&request.query);

    rows.into_iter()
        .filter(|row| {
            selected_row(
                &row.path,
                &row.language_id,
                row.is_generated,
                status,
                request,
            )
        })
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
            let usage_context_bonus = reference_row_usage_context_bonus(
                base_score,
                &row,
                focused_source_excerpt.as_deref(),
                request,
            );
            let score = base_score
                + usage_context_bonus
                + reference_source_path_bonus(
                    base_score,
                    &row.path,
                    request,
                    query_has_test_intent,
                )
                + reference_test_path_penalty(
                    base_score,
                    &row.path,
                    request,
                    query_has_test_intent,
                )
                + reference_same_name_file_penalty(base_score, &row.path, request);
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
                        is_generated: row.is_generated,
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

fn reference_row_usage_context_bonus(
    base_score: f64,
    row: &ReferenceRow,
    focused_source_excerpt: Option<&str>,
    request: &CodeRetrievalRequest,
) -> f64 {
    if row.kind == "type" {
        if let Some(bonus) = type_reference_row_usage_context_bonus(base_score, row, request) {
            return bonus;
        }
    }
    reference_usage_context_bonus(
        base_score,
        &row.kind,
        &row.name,
        focused_source_excerpt,
        request,
    )
}

fn type_reference_row_usage_context_bonus(
    base_score: f64,
    row: &ReferenceRow,
    request: &CodeRetrievalRequest,
) -> Option<f64> {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::References {
        return Some(0.0);
    }

    let source_excerpt = row.source_excerpt.as_deref()?;
    let line_start = row.source_excerpt_line_start?;
    let offset = usize::try_from(row.line_range.start.checked_sub(line_start)?).ok()?;
    let raw_lines = source_excerpt.lines().collect::<Vec<_>>();
    let target_line = source_usage_line(raw_lines.get(offset)?)?;
    identifier_ranges(target_line, &row.name).next()?;
    let previous_lines = raw_lines[..offset]
        .iter()
        .filter_map(|line| source_usage_line(line))
        .collect::<Vec<_>>();

    Some(
        reference_line_usage_bonus(
            target_line,
            &row.kind,
            &row.name,
            parameter_type_context(&previous_lines),
        )
        .unwrap_or(0.0),
    )
}

pub(super) fn reference_usage_context_bonus(
    base_score: f64,
    reference_kind: &str,
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

    let lines = source_excerpt
        .lines()
        .filter_map(source_usage_line)
        .collect::<Vec<_>>();

    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            reference_line_usage_bonus(
                line,
                reference_kind,
                name,
                parameter_type_context(&lines[..index]),
            )
        })
        .fold(0.0, f64::max)
}

fn source_usage_line(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with("//") || line.starts_with('*') {
        None
    } else {
        Some(line)
    }
}

fn reference_line_usage_bonus(
    line: &str,
    reference_kind: &str,
    name: &str,
    parameter_context: Option<ParameterTypeContext>,
) -> Option<f64> {
    identifier_ranges(line, name)
        .filter(|(start, end)| !line_declares_reference_name(line, name, *start, *end))
        .map(|(start, end)| {
            reference_identifier_usage_bonus(
                line,
                reference_kind,
                name,
                start,
                end,
                parameter_context,
            )
        })
        .max_by(f64::total_cmp)
        .filter(|bonus| *bonus > 0.0)
}

fn reference_identifier_usage_bonus(
    line: &str,
    reference_kind: &str,
    name: &str,
    start: usize,
    end: usize,
    parameter_context: Option<ParameterTypeContext>,
) -> f64 {
    let before = line.get(..start).unwrap_or_default();
    let after = line.get(end..).unwrap_or_default().trim_start();
    if reference_kind == "type" {
        if let Some(bonus) = type_reference_usage_bonus(line, before, name, parameter_context) {
            return bonus;
        }
    }
    if identifier_is_indirect_call(after) {
        return REFERENCE_INDIRECT_CALL_USAGE_BONUS;
    }
    if identifier_is_member_call(before, after) {
        return REFERENCE_MEMBER_CALL_USAGE_BONUS;
    }
    if identifier_is_plain_call(after) {
        return if identifier_is_return_value(before) {
            REFERENCE_RETURN_CALL_USAGE_BONUS
        } else {
            REFERENCE_PLAIN_CALL_USAGE_BONUS
        };
    }
    if identifier_is_assignment_value(before) {
        return REFERENCE_ASSIGNMENT_USAGE_BONUS;
    }
    if identifier_is_return_value(before) {
        return REFERENCE_RETURN_USAGE_BONUS;
    }

    0.0
}

fn line_declares_reference_name(line: &str, name: &str, start: usize, end: usize) -> bool {
    let before = line.get(..start).unwrap_or_default().trim_end();
    let after = line.get(end..).unwrap_or_default().trim_start();
    if before.ends_with('.')
        || before.ends_with("->")
        || identifier_is_assignment_value(before)
        || type_annotation_context_prefix(before).is_some()
    {
        return false;
    }
    if identifier_is_return_value(before) {
        return false;
    }
    if before.ends_with(':') {
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
    if prefix_ends_with_value_flow_keyword(before) {
        return false;
    }
    let token_count = before.split_whitespace().count();
    token_count >= 1
        && before
            .chars()
            .all(|character| !matches!(character, '=' | '+' | '-' | '*' | '/' | '%' | '?'))
}

fn prefix_ends_with_value_flow_keyword(before: &str) -> bool {
    before
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|token| !token.is_empty())
        .is_some_and(|token| matches!(token, "return" | "yield" | "await" | "new"))
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

fn identifier_is_return_value(before: &str) -> bool {
    before
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|token| !token.is_empty())
        .is_some_and(|token| matches!(token, "return" | "yield" | "await"))
}

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
