use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts,
    code_query_excerpts::reference_excerpt,
    code_query_path_ranking::{
        query_mentions_test_or_benchmark, reference_source_path_bonus, reference_test_path_penalty,
    },
    code_query_rows::ReferenceRow,
    code_query_support::*,
    code_search_plannable_outage_reason, dedupe_sort_truncate, hit_from_parts, mark_hits_degraded,
    prepare_code_search_statement, required_scope, selected_row,
};

const REFERENCE_ASSIGNMENT_USAGE_BONUS: f64 = 1.4;
const REFERENCE_INDIRECT_CALL_USAGE_BONUS: f64 = 1.8;
const REFERENCE_MEMBER_CALL_USAGE_BONUS: f64 = 1.2;
const REFERENCE_RETURN_USAGE_BONUS: f64 = 1.45;
const REFERENCE_PLAIN_CALL_USAGE_BONUS: f64 = 1.05;
const REFERENCE_RETURN_CALL_USAGE_BONUS: f64 = 1.55;
const REFERENCE_PARAMETER_TYPE_USAGE_BONUS: f64 = 0.45;
const REFERENCE_EXPORTED_PARAMETER_TYPE_USAGE_BONUS: f64 = 0.75;
const REFERENCE_MATCHING_PARAMETER_NAME_TYPE_BONUS: f64 = 0.65;
const REFERENCE_MATCHING_MULTILINE_PARAMETER_NAME_TYPE_BONUS: f64 = 0.25;
const REFERENCE_TYPE_USAGE_BONUS: f64 = 1.65;
const MAX_GENERIC_CALL_LOOKAHEAD_BYTES: usize = 256;

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

fn reference_same_name_file_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let file_stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    if normalized_identifier(file_stem) == normalized_identifier(&request.query) {
        -0.45
    } else {
        0.0
    }
}

fn normalized_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
        if let Some(annotation_prefix) = type_annotation_context_prefix(before) {
            return REFERENCE_TYPE_USAGE_BONUS
                + parameter_type_reference_bonus(line, annotation_prefix, name, parameter_context);
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

#[derive(Clone, Copy)]
struct ParameterTypeContext {
    exported_callable: bool,
}

fn parameter_type_context(previous_lines: &[&str]) -> Option<ParameterTypeContext> {
    let context = previous_lines.join("\n");
    let open_paren = context.rfind('(')?;
    if context[open_paren + 1..].contains(')') {
        return None;
    }
    let head_line = context[..open_paren]
        .lines()
        .next_back()
        .unwrap_or_default();

    Some(ParameterTypeContext {
        exported_callable: line_starts_exported_callable(head_line),
    })
}

fn parameter_type_reference_bonus(
    line: &str,
    before: &str,
    name: &str,
    parameter_context: Option<ParameterTypeContext>,
) -> f64 {
    let same_line_parameter = type_annotation_is_callable_parameter(before);
    let multiline_parameter = !same_line_parameter
        && parameter_context.is_some()
        && type_annotation_has_parameter_name(before);
    if !same_line_parameter && !multiline_parameter {
        return 0.0;
    }
    let callable_bonus = if line_starts_exported_callable(line)
        || parameter_context.is_some_and(|context| context.exported_callable)
    {
        REFERENCE_EXPORTED_PARAMETER_TYPE_USAGE_BONUS
    } else {
        REFERENCE_PARAMETER_TYPE_USAGE_BONUS
    };
    callable_bonus + matching_parameter_name_type_bonus(before, name, same_line_parameter)
}

fn type_annotation_is_callable_parameter(before: &str) -> bool {
    let before = before.trim_end();
    let Some(prefix) = before.strip_suffix(':') else {
        return false;
    };
    let Some(open_paren) = prefix.rfind('(') else {
        return false;
    };
    if prefix[open_paren + 1..].contains(')') {
        return false;
    }

    prefix[open_paren + 1..]
        .split(',')
        .next_back()
        .is_some_and(parameter_segment_has_name)
}

fn type_annotation_has_parameter_name(before: &str) -> bool {
    let before = before.trim_end();
    before
        .strip_suffix(':')
        .is_some_and(parameter_segment_has_name)
}

fn parameter_segment_has_name(segment: &str) -> bool {
    parameter_segment_name(segment).is_some_and(|name| !name.is_empty())
}

fn matching_parameter_name_type_bonus(before: &str, type_name: &str, same_line: bool) -> f64 {
    let Some(parameter_name) = type_annotation_parameter_name(before, same_line) else {
        return 0.0;
    };
    if !parameter_name_matches_type(&parameter_name, type_name) {
        return 0.0;
    }
    if same_line {
        REFERENCE_MATCHING_PARAMETER_NAME_TYPE_BONUS
    } else {
        REFERENCE_MATCHING_MULTILINE_PARAMETER_NAME_TYPE_BONUS
    }
}

fn type_annotation_parameter_name(before: &str, same_line: bool) -> Option<String> {
    let prefix = before.trim_end().strip_suffix(':')?;
    let segment = if same_line {
        let open_paren = prefix.rfind('(')?;
        prefix[open_paren + 1..].split(',').next_back()?
    } else {
        prefix
    };
    parameter_segment_name(segment)
}

fn parameter_segment_name(segment: &str) -> Option<String> {
    let segment = segment
        .trim()
        .trim_start_matches("readonly ")
        .trim_start_matches("public ")
        .trim_start_matches("private ")
        .trim_start_matches("protected ");
    let name = segment
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())?;
    name.chars()
        .next()
        .filter(|character| *character == '_' || character.is_ascii_alphabetic())?;
    Some(name.to_owned())
}

fn parameter_name_matches_type(parameter_name: &str, type_name: &str) -> bool {
    let parameter = normalized_identifier(parameter_name);
    parameter.len() >= 4 && normalized_identifier(type_name).contains(&parameter)
}

fn line_starts_exported_callable(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("export function ")
        || line.starts_with("export async function ")
        || (line.starts_with("export const ") && (line.contains("=>") || line.contains("function")))
}

fn line_declares_reference_name(line: &str, name: &str, start: usize, end: usize) -> bool {
    let before = line.get(..start).unwrap_or_default().trim_end();
    let after = line.get(end..).unwrap_or_default().trim_start();
    if before.ends_with('.')
        || before.ends_with("->")
        || identifier_is_assignment_value(before)
        || identifier_is_type_annotation(before)
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

fn identifier_is_type_annotation(before: &str) -> bool {
    let before = before.trim_end();
    before.ends_with(':') || before.ends_with(" as")
}

fn type_annotation_context_prefix(before: &str) -> Option<&str> {
    let before = before.trim_end();
    if identifier_is_type_annotation(before) {
        return Some(before);
    }
    if let Some(prefix) = nested_type_assertion_prefix(before) {
        return Some(prefix);
    }
    let colon_index = before.rfind(':')?;
    let suffix = before[colon_index + 1..].trim();
    nested_type_context_suffix(suffix).then_some(&before[..=colon_index])
}

fn nested_type_assertion_prefix(before: &str) -> Option<&str> {
    let assertion_index = before.rfind(" as ")?;
    let suffix = before[assertion_index + " as ".len()..].trim();
    nested_type_context_suffix(suffix).then_some(&before[..assertion_index + " as".len()])
}

fn nested_type_context_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .any(|character| matches!(character, '[' | '<' | '|' | ','))
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

fn identifier_is_plain_call(after: &str) -> bool {
    after.starts_with('(') || identifier_is_generic_call(after)
}

fn identifier_is_generic_call(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('<') else {
        return false;
    };
    let mut depth = 1usize;
    for (index, character) in rest.char_indices() {
        if index > MAX_GENERIC_CALL_LOOKAHEAD_BYTES {
            return false;
        }
        match character {
            '<' => depth = depth.saturating_add(1),
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let tail_start = index + character.len_utf8();
                    return rest
                        .get(tail_start..)
                        .unwrap_or_default()
                        .trim_start()
                        .starts_with('(');
                }
            }
            _ => {}
        }
    }

    false
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

    #[test]
    fn reference_usage_context_prioritizes_returns_and_function_type_annotations() {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        let request = CodeRetrievalRequest::new(
            "InstanceContext",
            selector,
            CodeQueryKind::References,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");

        let assignment = reference_usage_context_bonus(
            5.0,
            "value",
            "normalizeRoleId",
            Some("state.coordinatorRoleId = normalizeRoleId(roleId) || null;"),
            &request,
        );
        let returned = reference_usage_context_bonus(
            5.0,
            "value",
            "normalizeRoleId",
            Some("return normalizeRoleId(state.coordinatorRoleId);"),
            &request,
        );
        let type_signature = reference_usage_context_bonus(
            5.0,
            "type",
            "InstanceContext",
            Some("export function plan(input: Input, instance: InstanceContext) {"),
            &request,
        );
        let nested_type_signature = reference_usage_context_bonus(
            5.0,
            "type",
            "InstanceContext",
            Some("export function plan(input: Record<string, InstanceContext>) {"),
            &request,
        );

        assert!(returned > assignment);
        assert!(type_signature > 0.0);
        assert!(type_signature > nested_type_signature);
        assert!(
            reference_same_name_file_penalty(
                5.0,
                "packages/opencode/src/project/instance-context.ts",
                &request,
            ) < 0.0
        );
    }

    #[test]
    fn plain_call_detection_requires_real_generic_call_shape() {
        assert!(identifier_is_plain_call("(value)"));
        assert!(identifier_is_plain_call("<Payload>(value)"));
        assert!(identifier_is_plain_call("<Map<Key, Value>>(value)"));
        assert!(!identifier_is_plain_call("< computeThreshold())"));
        assert!(!identifier_is_plain_call("< bar(baz)"));
        assert!(!identifier_is_plain_call("<Payload> + value"));
    }
}
