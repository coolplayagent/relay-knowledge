#[cfg(test)]
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

#[path = "code_query_call_counts.rs"]
mod code_query_call_counts;
#[path = "code_query_call_direction.rs"]
mod code_query_call_direction;
#[path = "code_query_flow_scoring.rs"]
mod code_query_flow_scoring;
#[path = "code_query_identifiers.rs"]
mod code_query_identifiers;
#[path = "code_query_import_scoring.rs"]
mod code_query_import_scoring;
#[path = "code_query_import_targets.rs"]
mod code_query_import_targets;
#[path = "code_query_line_ranges.rs"]
mod code_query_line_ranges;
#[path = "code_query_path_ranking.rs"]
mod code_query_path_ranking;
#[path = "code_query_prepare.rs"]
mod code_query_prepare;
#[path = "code_query_rows.rs"]
mod code_query_rows;
#[path = "code_query_support.rs"]
mod code_query_support;
#[path = "code_query_symbols.rs"]
mod code_query_symbols;

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

#[cfg(test)]
const MAX_CANDIDATE_BIND_VALUES: usize = 900;

use super::code_query_hits::selected_row;
pub(super) use super::code_query_hits::{
    HitParts, chunk_layers, dedupe_sort_truncate, hit_from_parts, required_repository,
    required_scope,
};
#[cfg(test)]
use super::code_query_scope::path_matches_filter;
pub(super) use super::code_query_scope::{language_filter_allows, path_filter_allows};
use code_query_call_counts::{caller_target_call_counts, caller_target_call_key};
use code_query_call_direction::{
    call_direction_fts_filter_sql, fts_values_for_limited_with_language_and_call_direction,
};
use code_query_flow_scoring::{
    caller_context_density_bonus, compact_high_coverage_chunk_bonus, execution_flow_chunk_bonus,
    inline_construct_chunk_bonus,
};
use code_query_import_scoring::{
    hybrid_import_sparse_query_penalty, import_binding_context_bonus, import_line_priority,
    import_same_file_usage_bonus, import_surface_bonus, import_target_directory_bonus,
    import_target_symbol_bonus, query_looks_like_import_path,
};
#[cfg(test)]
use code_query_import_targets::target_symbol_import_query;
use code_query_import_targets::{
    attach_import_query_usage_context, attach_import_target_symbols,
    search_imports_by_target_symbols,
};
use code_query_line_ranges::{call_result_line_range, optional_line_range_with_symbol_context};
use code_query_path_ranking::{
    CallSiteQueryIntent, call_site_example_path_penalty, call_site_source_path_bonus,
    call_site_test_path_penalty, callee_member_context_bonus, caller_result_assignment_bonus,
    declaration_surface_path_bonus, import_test_path_penalty, query_mentions_example_or_sample,
    query_mentions_test_or_benchmark,
};
use code_query_prepare::{prepare_code_search_statement, retry_code_search_operation};
use code_query_rows::{CallRow, ChunkRow, ImportRow, ReferenceRow};
use code_query_support::*;
use code_query_symbols::search_symbols;

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    retry_code_search_operation(|| search_code_with_status(connection, &status, &request))
}

pub(super) fn search_code_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status =
        super::code_status::repository_scope_status_by_source_scope(connection, source_scope)?
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "code repository source scope '{source_scope}' is not indexed"
                ))
            })?;

    retry_code_search_operation(|| search_code_with_status(connection, &status, &request))
}

fn search_code_with_status(
    connection: &mut Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind == CodeQueryKind::Impact {
        return Err(StorageError::InvalidInput(
            "impact query kind requires repo impact with base/head refs".to_owned(),
        ));
    }
    let mut hits = Vec::new();
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Symbol | CodeQueryKind::Definition
    ) {
        hits.extend(search_symbols(connection, status, request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::References
    ) {
        hits.extend(search_references(connection, status, request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Callers | CodeQueryKind::Callees
    ) {
        hits.extend(search_calls(connection, status, request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Imports
    ) {
        hits.extend(search_imports(connection, status, request)?);
    }
    if matches!(request.code_query_kind, CodeQueryKind::Hybrid) {
        hits.extend(search_chunks(connection, status, request)?);
    }
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

fn search_references(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT r.file_id, r.path, f.language_id, r.name, r.kind,
               r.target_symbol_snapshot_id, r.byte_start, r.byte_end,
               r.line_start, r.line_end, r.target_hint, r.resolution_state,
               r.confidence_basis_points, r.confidence_tier, s.canonical_symbol_id
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.source_scope = r.source_scope AND f.path = r.path
        LEFT JOIN code_repository_symbols s
            ON s.source_scope = r.source_scope
           AND s.symbol_snapshot_id = r.target_symbol_snapshot_id
        WHERE r.source_scope = ?
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
        ORDER BY r.path ASC, r.line_start ASC
        LIMIT ?
        "
    );
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
        |row| {
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
            })
        },
    )?;
    let score_query = ScoreQuery::new(&request.query);
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_query.score([
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
                        excerpt: format!("{} reference to {}", row.kind, row.name),
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
        .collect())
}

fn search_calls(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let call_direction_filter = call_direction_fts_filter_sql(request);
    let sql = format!(
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
        ORDER BY c.path ASC, c.line_start ASC
        LIMIT ?
        "
    );
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
        |row| {
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
        },
    )?;
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
    let query_has_example_intent = query_mentions_example_or_sample(query);
    let call_site_query_intent = CallSiteQueryIntent {
        test_or_benchmark: query_has_test_intent,
        example_or_sample: query_has_example_intent,
    };
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let call_site_counts = (request.code_query_kind == CodeQueryKind::Callers)
        .then(|| caller_target_call_counts(&rows));

    Ok(rows
        .into_iter()
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
                        excerpt: call_excerpt(
                            row.caller_excerpt.as_deref(),
                            &caller,
                            &row.callee_name,
                        ),
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
        .collect())
}

fn search_imports(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND i.import_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'import'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Import),
            candidate_limit(request, CandidateLayer::Import),
        )),
        |row| {
            Ok(ImportRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                module: row.get(3)?,
                matched_symbol_name: None,
                target_symbol_names: None,
                same_file_query_usage_count: 0,
                line_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
                target_hint: row.get(6)?,
                resolution_state: row.get(7)?,
                confidence_basis_points: row.get(8)?,
                confidence_tier: row.get(9)?,
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let score_query = ScoreQuery::new(&request.query);
    let query_has_test_intent = query_mentions_test_or_benchmark(&request.query);
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    rows.extend(search_imports_by_target_symbols(
        connection, status, request,
    )?);
    attach_import_query_usage_context(connection, status, request, &mut rows)?;
    if request.code_query_kind == CodeQueryKind::Imports
        && query_looks_like_import_path(&request.query)
    {
        attach_import_target_symbols(connection, status, &mut rows)?;
    }

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let base_score = score_query.score([
                row.module.as_str(),
                row.target_hint.as_deref().unwrap_or_default(),
                row.matched_symbol_name.as_deref().unwrap_or_default(),
            ]) + score_exact_path(&query, &row.path)
                + scoped_identity_query_bonus(
                    &request.query,
                    [
                        row.target_hint.as_deref().unwrap_or_default(),
                        row.matched_symbol_name.as_deref().unwrap_or_default(),
                    ],
                )
                + import_target_symbol_bonus(
                    request.query.as_str(),
                    row.matched_symbol_name.as_deref(),
                );
            let score = base_score
                + import_same_file_usage_bonus(
                    base_score,
                    row.same_file_query_usage_count,
                    request.code_query_kind,
                )
                + import_target_directory_bonus(
                    base_score,
                    &request.query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_binding_context_bonus(
                    base_score,
                    &request.query,
                    &row.module,
                    request.code_query_kind,
                )
                + import_line_priority(base_score, row.line_range.start, &request.query)
                + hybrid_import_sparse_query_penalty(
                    base_score,
                    &request.query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    row.matched_symbol_name.as_deref(),
                    request.code_query_kind,
                )
                + import_test_path_penalty(base_score, &row.path, request, query_has_test_intent)
                + import_surface_bonus(base_score, &row.path);
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: RepositoryCodeRange { start: 0, end: 0 },
                        line_range: row.line_range,
                        symbol_snapshot_id: None,
                        canonical_symbol_id: None,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
                        score: score + 1.0,
                        excerpt: import_excerpt(&row.module, row.target_symbol_names.as_deref()),
                        degraded_reason: None,
                        edge_kind: Some("import".to_owned()),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
                    },
                )
            })
        })
        .collect())
}

fn import_excerpt(module: &str, target_symbol_names: Option<&str>) -> String {
    let Some(target_symbol_names) = target_symbol_names
        .map(str::trim)
        .filter(|target_symbol_names| !target_symbol_names.is_empty())
    else {
        return module.to_owned();
    };

    format!("{module} target symbols: {target_symbol_names}")
}

fn search_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = hybrid_chunk_fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id,
               symbol.canonical_symbol_id, symbol.name, symbol.qualified_name,
               f.parse_status, f.degraded_reason
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols symbol
            ON symbol.source_scope = c.source_scope
           AND symbol.symbol_snapshot_id = c.symbol_snapshot_id
        WHERE c.source_scope = ?
          AND c.chunk_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'chunk'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY c.path ASC, c.line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Chunk),
            candidate_limit(request, CandidateLayer::Chunk),
        )),
        |row| {
            Ok(ChunkRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                content: row.get(3)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
                symbol_snapshot_id: row.get(8)?,
                canonical_symbol_id: row.get(9)?,
                symbol_name: row.get(10)?,
                symbol_qualified_name: row.get(11)?,
                parse_status: row.get(12)?,
                degraded_reason: row.get(13)?,
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let score_query = ScoreQuery::new(&request.query);
    let declaration_terms = query_terms(&query);
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let declaration_bonus = declaration_chunk_bonus(&declaration_terms, &row.content);
            let symbol_bonus = row.symbol_name.as_deref().map_or(0.0, |name| {
                symbol_query_bonus(
                    &request.query,
                    name,
                    row.symbol_qualified_name.as_deref().unwrap_or_default(),
                    "",
                    row.canonical_symbol_id.as_deref().unwrap_or_default(),
                    request,
                )
            });
            let score = score_query.score([&row.content, &row.path])
                + declaration_bonus
                + declaration_surface_path_bonus(declaration_bonus, &row.path, request)
                + symbol_bonus;
            let score = score
                + compact_high_coverage_chunk_bonus(
                    score,
                    &request.query,
                    &row.content,
                    &row.path,
                    request,
                )
                + execution_flow_chunk_bonus(
                    score,
                    &request.query,
                    &row.content,
                    &row.path,
                    request,
                )
                + inline_construct_chunk_bonus(
                    score,
                    &request.query,
                    &row.content,
                    &row.path,
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
                        symbol_snapshot_id: row.symbol_snapshot_id,
                        canonical_symbol_id: row.canonical_symbol_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: chunk_layers(&row.parse_status),
                        score,
                        excerpt: row.content,
                        degraded_reason: row.degraded_reason,
                        edge_kind: None,
                        edge_resolution_state: None,
                        edge_target_hint: None,
                        edge_confidence_basis_points: None,
                        edge_confidence_tier: None,
                    },
                )
            })
        })
        .collect())
}

#[cfg(test)]
#[path = "code_query_unit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "code_query_score_tests.rs"]
mod score_tests;

#[cfg(test)]
#[path = "code_query_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "code_query_call_ranking_tests.rs"]
mod call_ranking_tests;

#[cfg(test)]
#[path = "code_query_chunk_ranking_tests.rs"]
mod chunk_ranking_tests;

#[cfg(test)]
#[path = "code_query_symbol_ranking_tests.rs"]
mod symbol_ranking_tests;

#[cfg(test)]
#[path = "code_query_excerpt_tests.rs"]
mod excerpt_tests;
