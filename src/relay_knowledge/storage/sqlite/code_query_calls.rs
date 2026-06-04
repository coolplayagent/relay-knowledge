use std::collections::BTreeMap;

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
    code_query_call_counts::{caller_target_call_counts, caller_target_call_key},
    code_query_call_direction::{
        call_direction_fts_filter_sql, fts_values_for_limited_with_language_and_call_direction,
    },
    code_query_call_target_ranking::high_confidence_inferred_target_bonus,
    code_query_excerpts::{call_excerpt, callee_excerpt, line_declares_local_callable},
    code_query_flow_scoring::caller_context_density_bonus,
    code_query_line_ranges::{call_result_line_range, optional_line_range_with_symbol_context},
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

struct CallIdentityRows {
    rows: Vec<CallRow>,
    saturated: bool,
}

struct IndirectCallBinding {
    field_name: String,
    target_name: String,
    binding_path: String,
}

struct IndirectCallBindings {
    bindings: Vec<IndirectCallBinding>,
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
const LOCAL_CALLABLE_DECLARATION_BONUS: f64 = 1.8;
const INDIRECT_CALL_BINDING_LIMIT: usize = 80;
const MAX_INDIRECT_CALL_FIELDS: usize = 24;

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
        identity_hits = call_rows_to_hits(status, request, rows);
        let mut saturated = saturated;
        if direct_hit_count == 0 && request.code_query_kind == CodeQueryKind::Callers {
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
    let direct_limit = call_identity_candidate_limit(request);
    let sql = call_rows_sql(&format!(
        "
          AND {} = ?
          {path_filter}
          {language_filter}
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

fn search_indirect_call_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> Result<CallIdentityRows, StorageError> {
    if identity.direction != CallIdentityDirection::Callee {
        return Ok(CallIdentityRows {
            rows: Vec::new(),
            saturated: false,
        });
    }
    let bindings =
        search_indirect_call_bindings(connection, status, request, identity.leaf_name())?;
    if bindings.bindings.is_empty() {
        return Ok(CallIdentityRows {
            rows: Vec::new(),
            saturated: bindings.saturated,
        });
    }

    let mut field_names = Vec::new();
    for binding in &bindings.bindings {
        if !field_names.contains(&binding.field_name) {
            field_names.push(binding.field_name.clone());
        }
        if field_names.len() >= MAX_INDIRECT_CALL_FIELDS {
            break;
        }
    }

    let path_filter = path_filter_sql_for_column("c.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let placeholders = placeholders(field_names.len());
    let sql = call_rows_sql(&format!(
        "
          AND c.callee_name IN ({placeholders})
          {path_filter}
          {language_filter}
        "
    ));
    let direct_limit = call_identity_candidate_limit(request);
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(field_names.into_iter().map(Value::Text));
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
    for row in &mut rows {
        if let Some(binding) =
            best_indirect_call_binding(&bindings.bindings, &row.callee_name, &row.path)
        {
            row.target_hint = Some(binding.target_name.clone());
            row.resolution_state = "inferred".to_owned();
            row.confidence_basis_points = if row.path == binding.binding_path {
                7_500
            } else {
                5_500
            };
            row.confidence_tier = "inferred".to_owned();
        }
    }

    Ok(CallIdentityRows {
        rows,
        saturated: saturated || bindings.saturated,
    })
}

fn search_indirect_call_bindings(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    target_name: &str,
) -> Result<IndirectCallBindings, StorageError> {
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT path, content
        FROM code_repository_search
        WHERE code_repository_search MATCH ?
          AND source_scope = ?
          AND document_kind = 'chunk'
          {fts_filter}
        ORDER BY bm25(code_repository_search) ASC, record_id ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        Value::Text(fts_match_query(target_name)),
        Value::Text(required_scope(status)?.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((INDIRECT_CALL_BINDING_LIMIT + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > INDIRECT_CALL_BINDING_LIMIT;
    rows.truncate(INDIRECT_CALL_BINDING_LIMIT);
    let mut bindings: Vec<IndirectCallBinding> = Vec::new();
    for (path, excerpt) in rows {
        for field_name in indirect_call_binding_fields(&excerpt, target_name) {
            let binding = IndirectCallBinding {
                field_name,
                target_name: target_name.to_owned(),
                binding_path: path.clone(),
            };
            if !bindings.iter().any(|existing| {
                existing.field_name == binding.field_name
                    && existing.binding_path == binding.binding_path
            }) {
                bindings.push(binding);
            }
        }
    }

    Ok(IndirectCallBindings {
        bindings,
        saturated,
    })
}

fn best_indirect_call_binding<'a>(
    bindings: &'a [IndirectCallBinding],
    field_name: &str,
    call_path: &str,
) -> Option<&'a IndirectCallBinding> {
    bindings
        .iter()
        .find(|binding| binding.field_name == field_name && binding.binding_path == call_path)
        .or_else(|| {
            bindings
                .iter()
                .find(|binding| binding.field_name == field_name)
        })
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

fn indirect_call_binding_fields(excerpt: &str, target_name: &str) -> Vec<String> {
    let mut fields = Vec::new();
    for line in excerpt.lines() {
        if !line_contains_identifier(line, target_name) {
            continue;
        }
        if let Some(field_name) = field_name_before_bound_target(line, target_name)
            && !fields.contains(&field_name)
        {
            fields.push(field_name);
        }
    }

    fields
}

fn field_name_before_bound_target(line: &str, target_name: &str) -> Option<String> {
    let target_start = identifier_start(line, target_name)?;
    let before_target = line.get(..target_start)?;
    let assignment_start = before_target
        .rfind('=')
        .or_else(|| before_target.rfind(':'))?;
    let left = before_target.get(..assignment_start)?.trim_end();
    field_name_from_member_surface(left).filter(|field_name| field_name != target_name)
}

fn field_name_from_member_surface(value: &str) -> Option<String> {
    if let Some((_, tail)) = value.rsplit_once("->") {
        return leading_identifier(tail.trim_start());
    }
    if let Some((_, tail)) = value.rsplit_once('.') {
        return leading_identifier(tail.trim_start());
    }

    None
}

fn leading_identifier(value: &str) -> Option<String> {
    let mut end = 0usize;
    for (index, character) in value.char_indices() {
        if index == 0 && !identifier_start_character(character) {
            return None;
        }
        if !identifier_character(character) {
            break;
        }
        end = index + character.len_utf8();
    }
    (end > 0).then(|| value[..end].to_owned())
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    identifier_start(line, identifier).is_some()
}

fn identifier_start(line: &str, identifier: &str) -> Option<usize> {
    if identifier.is_empty() {
        return None;
    }
    line.match_indices(identifier)
        .find(|(start, _)| {
            let end = start + identifier.len();
            line.get(..*start).is_some_and(|prefix| {
                prefix
                    .chars()
                    .next_back()
                    .is_none_or(|character| !identifier_character(character))
            }) && line.get(end..).is_some_and(|suffix| {
                suffix
                    .chars()
                    .next()
                    .is_none_or(|character| !identifier_character(character))
            })
        })
        .map(|(start, _)| start)
}

fn identifier_start_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn search_call_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CallRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let call_direction_filter = call_direction_fts_filter_sql(request);
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
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
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
               caller_chunk.content
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols caller
            ON caller.source_scope = c.source_scope
           AND caller.symbol_snapshot_id = c.caller_symbol_snapshot_id
        LEFT JOIN code_repository_chunks caller_chunk
            ON caller_chunk.source_scope = c.source_scope
           AND caller_chunk.symbol_snapshot_id = c.caller_symbol_snapshot_id
           AND caller_chunk.line_start <= c.line_start
           AND caller_chunk.line_end >= c.line_start
        LEFT JOIN code_repository_symbols callee
            ON callee.source_scope = c.source_scope
           AND callee.symbol_snapshot_id = c.callee_symbol_snapshot_id
        WHERE c.source_scope = ?
          {predicate_sql}
        ORDER BY c.path ASC, c.line_start ASC
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
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
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
                let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
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
                        excerpt: if request.code_query_kind == CodeQueryKind::Callees {
                            callee_excerpt(row.caller_excerpt.as_deref(), &caller, &row.callee_name)
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

fn local_callable_declaration_bonus(
    base_score: f64,
    caller_excerpt: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    if caller_excerpt
        .lines()
        .any(|line| line_declares_local_callable(line, callee_name))
    {
        LOCAL_CALLABLE_DECLARATION_BONUS
    } else {
        0.0
    }
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
    let selector_is_narrow = !request.repository.path_filters.is_empty()
        || !request.repository.language_filters.is_empty();
    hit_count > 0
        && !saturated
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && (specific_call_identity_leaf(identity.leaf_name()) || selector_is_narrow)))
}

fn call_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Call).min(200)
}

fn specific_call_identity_leaf(leaf_name: &str) -> bool {
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
    fn caller_identity_fast_path_requires_bounded_exact_target_hits() {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        let callers_request = CodeRetrievalRequest::new(
            "TargetThing",
            selector.clone(),
            CodeQueryKind::Callers,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let callees_request = CodeRetrievalRequest::new(
            "TargetThing",
            selector,
            CodeQueryKind::Callees,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let callers_identity =
            call_identity_query(&callers_request).expect("callers identity should parse");
        let callees_identity =
            call_identity_query(&callees_request).expect("callees identity should parse");

        assert!(call_identity_hits_can_answer_without_fts(
            &callers_request,
            &callers_identity,
            3,
            false
        ));
        assert!(!call_identity_hits_can_answer_without_fts(
            &callers_request,
            &callers_identity,
            11,
            false
        ));
        assert!(!call_identity_hits_can_answer_without_fts(
            &callers_request,
            &callers_identity,
            3,
            true
        ));
        assert!(call_identity_hits_can_answer_without_fts(
            &callees_request,
            &callees_identity,
            3,
            false
        ));
        let broad_identity = call_identity_query(
            &CodeRetrievalRequest::new(
                "Table",
                CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
                    .expect("selector should validate"),
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .expect("identity query should parse");
        assert!(!call_identity_hits_can_answer_without_fts(
            &callees_request,
            &broad_identity,
            1,
            false
        ));

        let narrowed_selector = CodeRepositorySelector::new(
            "repo",
            "commit",
            vec!["src/table.cc".to_owned()],
            vec!["cpp".to_owned()],
        )
        .expect("selector should validate");
        let narrowed_request = CodeRetrievalRequest::new(
            "Run",
            narrowed_selector,
            CodeQueryKind::Callees,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let narrowed_identity =
            call_identity_query(&narrowed_request).expect("identity query should parse");

        assert!(call_identity_hits_can_answer_without_fts(
            &narrowed_request,
            &narrowed_identity,
            2,
            false
        ));
    }
}
