use std::collections::BTreeMap;

use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

#[cfg(test)]
use super::code_query_local_callable_scoring::LOCAL_CALLABLE_DECLARATION_BONUS;
use super::{
    HitParts,
    code_query_ambiguous_callees::search_ambiguous_callee_implementation_hits,
    code_query_call_counts::{caller_target_call_counts, caller_target_call_key},
    code_query_call_direction::{
        call_direction_fts_filter_sql, fts_values_for_limited_with_language_and_call_direction,
    },
    code_query_call_identity_support::{has_case_boundary, specific_call_identity_leaf},
    code_query_call_site_scoring::exact_caller_named_receiver_member_call_bonus,
    code_query_call_target_ranking::high_confidence_inferred_target_bonus,
    code_query_caller_context_scoring::caller_context_density_bonus,
    code_query_excerpts::{call_excerpt, callee_excerpt},
    code_query_line_ranges::{call_result_line_range, optional_line_range_with_symbol_context},
    code_query_local_callable_scoring::local_callable_declaration_bonus,
    code_query_path_ranking::{
        CallSiteQueryIntent, call_site_example_path_penalty, call_site_source_path_bonus,
        call_site_test_path_penalty, callee_member_context_bonus, caller_result_assignment_bonus,
        query_mentions_example_or_sample, query_mentions_test_or_benchmark,
    },
    code_query_rows::CallRow,
    code_query_support::*,
    dedupe_sort_truncate, hit_from_parts, prepare_code_search_statement, required_scope,
    selected_row,
};

#[path = "code_query_indirect_calls.rs"]
mod code_query_indirect_calls;

use code_query_indirect_calls::search_indirect_call_identity_rows;

struct CallIdentityRows {
    rows: Vec<CallRow>,
    saturated: bool,
}

struct CallIdentityQuery {
    direction: CallIdentityDirection,
    symbol: SymbolIdentityQuery,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallIdentityDirection {
    Caller,
    Callee,
}

type CalleeExecutionGroupKey = (String, String, u32, u32);
type CalleeExecutionSiteKey = (CalleeExecutionGroupKey, u32, u32, String, String);
type CalleeExecutionOrder = BTreeMap<CalleeExecutionSiteKey, (usize, usize)>;

const CALLEE_EXECUTION_ORDER_STEP: f64 = 0.18;

pub(super) fn search_calls(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = call_identity_query(request);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        let identity_rows = search_call_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| identity.matches_row(row))
            .collect::<Vec<_>>();
        let direct_hit_count = rows.len();
        let implementation_hits =
            search_ambiguous_callee_implementation_hits(connection, status, request, &rows)?;
        identity_hits = call_rows_to_hits(status, request, rows);
        identity_hits.extend(implementation_hits);
        let mut saturated = saturated;
        if request.code_query_kind == CodeQueryKind::Callers
            && !saturated
            && (direct_hit_count == 0
                || call_identity_leaf_or_selector_is_specific(request, identity))
        {
            let indirect_rows =
                search_indirect_call_identity_rows(connection, status, request, identity)?;
            saturated = saturated || indirect_rows.saturated;
            identity_hits.extend(call_rows_to_hits(status, request, indirect_rows.rows));
        }
        if call_identity_hits_can_answer_without_fts(
            request,
            identity,
            identity_hits.len(),
            saturated,
        ) {
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    }

    let mut hits = call_rows_to_hits(
        status,
        request,
        search_call_fts_rows(connection, status, request)?,
    );
    hits.extend(identity_hits);

    Ok(hits)
}

fn search_call_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> Result<CallIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("c.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let direct_limit = call_identity_candidate_limit(request);
    let sql = call_rows_sql(&format!(
        "
          AND {} = ?
          {path_filter}
          {language_filter}
          {generated_filter}
        ",
        identity.match_column()
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
    let rows = statement.query_map(params_from_iter(values), row_to_call)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(CallIdentityRows { rows, saturated })
}

impl CallIdentityQuery {
    fn leaf_name(&self) -> &str {
        self.symbol.leaf_name()
    }

    fn is_scoped(&self) -> bool {
        self.symbol.is_scoped()
    }

    fn match_column(&self) -> &'static str {
        match self.direction {
            CallIdentityDirection::Caller => "c.caller_name",
            CallIdentityDirection::Callee => "c.callee_name",
        }
    }

    fn matches_row(&self, row: &CallRow) -> bool {
        match self.direction {
            CallIdentityDirection::Caller => self.symbol.matches_symbol(
                row.caller_name.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
                row.caller_signature.as_deref().unwrap_or_default(),
                row.caller_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ),
            CallIdentityDirection::Callee => self.symbol.matches_symbol(
                &row.callee_name,
                row.target_hint.as_deref().unwrap_or_default(),
                row.callee_signature.as_deref().unwrap_or_default(),
                row.callee_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ),
        }
    }
}

fn search_call_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CallRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let call_direction_filter = call_direction_fts_filter_sql(request);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = call_rows_sql(&format!(
        "
          AND c.call_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'call'
                {fts_filter}
                {call_direction_filter}
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
        params_from_iter(fts_values_for_limited_with_language_and_call_direction(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Call),
            candidate_limit(request, CandidateLayer::Call),
        )),
        row_to_call,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn call_rows_sql(predicate_sql: &str) -> String {
    format!(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_symbol_snapshot_id, c.callee_name,
               c.line_start, c.line_end, caller.line_start, caller.line_end,
               (
                   SELECT MAX(previous.line_end)
                   FROM code_repository_symbols previous
                   WHERE previous.source_scope = c.source_scope
                     AND previous.path = caller.path
                     AND caller.line_start IS NOT NULL
                     AND previous.line_end < caller.line_start
               ) AS caller_previous_symbol_line_end,
               c.target_hint, c.resolution_state,
               c.confidence_basis_points, c.confidence_tier,
               caller.canonical_symbol_id, callee.canonical_symbol_id,
               caller.signature, callee.signature,
               (
                   SELECT chunk.content
                   FROM code_repository_chunks chunk
                   WHERE chunk.source_scope = c.source_scope
                     AND chunk.symbol_snapshot_id = c.caller_symbol_snapshot_id
                     AND chunk.line_start <= c.line_start
                     AND chunk.line_end >= c.line_start
                   ORDER BY (chunk.line_end - chunk.line_start) DESC,
                            chunk.line_start ASC,
                            chunk.chunk_id ASC
                   LIMIT 1
               ) AS caller_excerpt,
               (
                   SELECT chunk.content
                   FROM code_repository_chunks chunk
                   WHERE chunk.source_scope = c.source_scope
                     AND chunk.symbol_snapshot_id = c.callee_symbol_snapshot_id
                   ORDER BY (chunk.line_end - chunk.line_start) DESC,
                            chunk.line_start ASC,
                            chunk.chunk_id ASC
                   LIMIT 1
               ) AS callee_excerpt,
               f.is_generated
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols caller
            ON caller.source_scope = c.source_scope
           AND caller.symbol_snapshot_id = c.caller_symbol_snapshot_id
        LEFT JOIN code_repository_symbols callee
            ON callee.source_scope = c.source_scope
           AND callee.symbol_snapshot_id = c.callee_symbol_snapshot_id
        WHERE c.source_scope = ?
          {predicate_sql}
        ORDER BY f.is_generated ASC, c.path ASC, c.line_start ASC
        LIMIT ?
        "
    )
}

fn row_to_call(row: &Row<'_>) -> rusqlite::Result<CallRow> {
    Ok(CallRow {
        file_id: row.get(0)?,
        path: row.get(1)?,
        language_id: row.get(2)?,
        caller_symbol_snapshot_id: row.get(3)?,
        caller_name: row.get(4)?,
        callee_symbol_snapshot_id: row.get(5)?,
        callee_name: row.get(6)?,
        line_range: RepositoryCodeRange {
            start: row.get(7)?,
            end: row.get(8)?,
        },
        caller_line_range: optional_line_range_with_symbol_context(
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
        ),
        target_hint: row.get(12)?,
        resolution_state: row.get(13)?,
        confidence_basis_points: row.get(14)?,
        confidence_tier: row.get(15)?,
        caller_canonical_symbol_id: row.get(16)?,
        callee_canonical_symbol_id: row.get(17)?,
        caller_signature: row.get(18)?,
        callee_signature: row.get(19)?,
        caller_excerpt: row.get(20)?,
        callee_excerpt: row.get(21)?,
        is_generated: row.get::<_, i64>(22)? != 0,
    })
}

fn call_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<CallRow>,
) -> Vec<CodeRetrievalHit> {
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
    let query_has_example_intent = query_mentions_example_or_sample(query);
    let call_site_query_intent = CallSiteQueryIntent {
        test_or_benchmark: query_has_test_intent,
        example_or_sample: query_has_example_intent,
    };
    let call_site_counts = (request.code_query_kind == CodeQueryKind::Callers)
        .then(|| caller_target_call_counts(&rows));
    let callee_execution_order = callee_execution_order(&rows, request);

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
            let caller_target_call_count = call_site_counts
                .as_ref()
                .and_then(|counts| {
                    caller_target_call_key(&row).and_then(|key| counts.get(&key).copied())
                })
                .unwrap_or(1);
            let caller_name = row.caller_name.as_deref().unwrap_or_default();
            let target_hint = row.target_hint.as_deref().unwrap_or_default();
            let caller_canonical_id = row
                .caller_canonical_symbol_id
                .as_deref()
                .unwrap_or_default();
            let callee_canonical_id = row
                .callee_canonical_symbol_id
                .as_deref()
                .unwrap_or_default();
            let (base_score, scoped_identity_bonus) = match request.code_query_kind {
                CodeQueryKind::Callees => (
                    score_query.score([
                        caller_name,
                        caller_canonical_id,
                        row.caller_signature.as_deref().unwrap_or_default(),
                    ]),
                    scoped_identity_query_bonus(query, [caller_canonical_id]),
                ),
                CodeQueryKind::Callers => (
                    score_query.score([
                        row.callee_name.as_str(),
                        target_hint,
                        callee_canonical_id,
                        row.callee_signature.as_deref().unwrap_or_default(),
                    ]),
                    scoped_identity_query_bonus(query, [target_hint, callee_canonical_id]),
                ),
                _ => (
                    score_query.score([
                        caller_name,
                        row.callee_name.as_str(),
                        target_hint,
                        caller_canonical_id,
                        callee_canonical_id,
                        row.caller_signature.as_deref().unwrap_or_default(),
                        row.callee_signature.as_deref().unwrap_or_default(),
                    ]),
                    scoped_identity_query_bonus(
                        query,
                        [target_hint, caller_canonical_id, callee_canonical_id],
                    ),
                ),
            };
            let source_path_bonus = call_site_source_path_bonus(
                base_score,
                &row.path,
                request,
                query,
                query_has_test_intent,
            );
            let test_path_penalty =
                call_site_test_path_penalty(base_score, &row.path, request, query_has_test_intent);
            let example_path_penalty = call_site_example_path_penalty(
                base_score,
                &row.path,
                request,
                query_has_example_intent,
            );
            let repeated_site_bonus =
                if test_path_penalty >= 0.0 && (source_path_bonus > 0.0 || query_has_test_intent) {
                    repeated_call_site_bonus(base_score, caller_target_call_count, request)
                } else {
                    0.0
                };
            let score = base_score
                + scoped_identity_bonus
                + directional_call_context_bonus(
                    &score_query,
                    base_score,
                    row.caller_name.as_deref(),
                    &row.callee_name,
                    &row.path,
                    request,
                )
                + callee_member_context_bonus(
                    base_score,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + exact_caller_named_receiver_member_call_bonus(
                    base_score,
                    query,
                    row.caller_name.as_deref(),
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + caller_result_assignment_bonus(
                    base_score,
                    &row.path,
                    query,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                    call_site_query_intent,
                )
                + high_confidence_inferred_target_bonus(
                    base_score,
                    query,
                    &row.callee_name,
                    target_hint,
                    &row.resolution_state,
                    row.confidence_basis_points,
                    request,
                )
                + same_named_caller_penalty(row.caller_name.as_deref(), &row.callee_name, request)
                + caller_context_density_bonus(
                    base_score,
                    query,
                    row.caller_name.as_deref(),
                    &row.callee_name,
                    &row.path,
                    row.caller_excerpt.as_deref(),
                    request,
                )
                + local_callable_declaration_bonus(
                    base_score,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + callee_execution_order_bonus(&callee_execution_order, &row, request)
                + repeated_site_bonus
                + callee_related_name_bonus(query, &row.callee_name, request);
            let score = score + source_path_bonus + test_path_penalty + example_path_penalty;
            (score > 0.0).then(|| {
                let line_range = call_result_line_range(request.code_query_kind, &row);
                let caller = call_display_name(
                    row.caller_name.as_deref(),
                    row.caller_canonical_symbol_id.as_deref(),
                )
                .or_else(|| inferred_caller_name_from_excerpt(row.caller_excerpt.as_deref()))
                .unwrap_or_else(|| "<module>".to_owned());
                let (symbol_snapshot_id, canonical_symbol_id) =
                    if request.code_query_kind == CodeQueryKind::Callees {
                        (
                            row.callee_symbol_snapshot_id,
                            row.callee_canonical_symbol_id,
                        )
                    } else {
                        (
                            row.caller_symbol_snapshot_id,
                            row.caller_canonical_symbol_id,
                        )
                    };
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range,
                        symbol_snapshot_id,
                        canonical_symbol_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
                        score: score
                            + 1.25
                            + call_edge_confidence_bonus(row.confidence_basis_points),
                        is_generated: row.is_generated,
                        excerpt: if request.code_query_kind == CodeQueryKind::Callees {
                            callee_excerpt(
                                row.caller_excerpt.as_deref(),
                                row.callee_excerpt.as_deref(),
                                &caller,
                                &row.callee_name,
                            )
                        } else {
                            call_excerpt(row.caller_excerpt.as_deref(), &caller, &row.callee_name)
                        },
                        degraded_reason: None,
                        edge_kind: Some("call".to_owned()),
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

fn callee_execution_order(
    rows: &[CallRow],
    request: &CodeRetrievalRequest,
) -> CalleeExecutionOrder {
    if request.code_query_kind != CodeQueryKind::Callees {
        return BTreeMap::new();
    }

    let mut grouped = BTreeMap::<CalleeExecutionGroupKey, Vec<CalleeExecutionSiteKey>>::new();
    for row in rows {
        let Some(group_key) = callee_execution_group_key(row) else {
            continue;
        };
        let site_key = callee_execution_site_key(group_key.clone(), row);
        grouped.entry(group_key).or_default().push(site_key);
    }

    let mut order = BTreeMap::new();
    for sites in grouped.values_mut() {
        sites.sort();
        sites.dedup();
        if sites.len() <= 1 {
            continue;
        }
        let site_count = sites.len();
        for (position, site) in sites.iter().cloned().enumerate() {
            order.insert(site, (position, site_count));
        }
    }

    order
}

fn callee_execution_group_key(row: &CallRow) -> Option<CalleeExecutionGroupKey> {
    let caller = row
        .caller_symbol_snapshot_id
        .as_deref()
        .or(row.caller_name.as_deref())?;
    let (caller_start, caller_end) = row
        .caller_line_range
        .as_ref()
        .map_or((0, 0), |range| (range.start, range.end));

    Some((
        row.path.clone(),
        caller.to_owned(),
        caller_start,
        caller_end,
    ))
}

fn callee_execution_site_key(
    group_key: CalleeExecutionGroupKey,
    row: &CallRow,
) -> CalleeExecutionSiteKey {
    (
        group_key,
        row.line_range.start,
        row.line_range.end,
        row.callee_name.clone(),
        row.target_hint.clone().unwrap_or_default(),
    )
}

fn callee_execution_order_bonus(
    order: &CalleeExecutionOrder,
    row: &CallRow,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let Some(group_key) = callee_execution_group_key(row) else {
        return 0.0;
    };
    let site_key = callee_execution_site_key(group_key, row);
    let Some((position, site_count)) = order.get(&site_key) else {
        return 0.0;
    };

    site_count.saturating_sub(*position).min(5) as f64 * CALLEE_EXECUTION_ORDER_STEP
}

fn call_display_name(name: Option<&str>, canonical_symbol_id: Option<&str>) -> Option<String> {
    let canonical_symbol_id = canonical_symbol_id
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let terms = canonical_symbol_id
        .map(display_identity_terms)
        .unwrap_or_default();
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| terms.last().copied())?;
    if terms.is_empty() {
        return Some(name.to_owned());
    }
    let name_index = terms.iter().rposition(|term| *term == name)?;
    let owner = terms.get(name_index.checked_sub(1)?)?;
    if *owner == name || !display_owner_term(owner) || !generic_nested_display_name(name) {
        return Some(name.to_owned());
    }

    Some(format!("{owner}.{name}"))
}

fn generic_nested_display_name(name: &str) -> bool {
    matches!(
        name,
        "callback"
            | "client"
            | "connection"
            | "event"
            | "handler"
            | "item"
            | "request"
            | "response"
            | "source"
            | "stream"
            | "value"
    )
}

fn inferred_caller_name_from_excerpt(caller_excerpt: Option<&str>) -> Option<String> {
    caller_excerpt?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(caller_name_from_declaration_line)
}

fn caller_name_from_declaration_line(line: &str) -> Option<String> {
    let open = line.find('(')?;
    let before_open = line[..open].trim_end();
    let end = before_open
        .char_indices()
        .rev()
        .find(|(_, character)| identifier_character(*character))
        .map(|(index, character)| index + character.len_utf8())?;
    let start = before_open[..end]
        .char_indices()
        .rev()
        .find(|(_, character)| !identifier_character(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let name = before_open[start..end].trim();
    if name.is_empty() || declaration_caller_name_is_control_keyword(name) {
        None
    } else {
        Some(name.to_owned())
    }
}

fn declaration_caller_name_is_control_keyword(name: &str) -> bool {
    matches!(name, "catch" | "for" | "if" | "switch" | "while" | "with")
}

fn identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn display_owner_term(term: &str) -> bool {
    term.chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        || has_case_boundary(term)
        || term.contains('_')
}

fn display_identity_terms(value: &str) -> Vec<&str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .collect()
}

fn call_identity_query(request: &CodeRetrievalRequest) -> Option<CallIdentityQuery> {
    let direction = match request.code_query_kind {
        CodeQueryKind::Callers => CallIdentityDirection::Callee,
        CodeQueryKind::Callees => CallIdentityDirection::Caller,
        _ => return None,
    };
    let symbol = SymbolIdentityQuery::from_query(&request.query)?;

    Some(CallIdentityQuery { direction, symbol })
}

fn call_identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && call_identity_leaf_or_selector_is_specific(request, identity)))
}

fn call_identity_leaf_or_selector_is_specific(
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> bool {
    specific_call_identity_leaf(identity.leaf_name())
        || !request.repository.path_filters.is_empty()
        || !request.repository.language_filters.is_empty()
}

fn call_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Call).min(200)
}

#[cfg(test)]
#[path = "code_query_call_core_tests.rs"]
mod tests;
