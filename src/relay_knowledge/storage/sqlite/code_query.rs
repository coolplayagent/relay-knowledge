use std::collections::BTreeMap;

#[cfg(test)]
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

#[path = "code_query_call_counts.rs"]
mod code_query_call_counts;
#[path = "code_query_import_targets.rs"]
mod code_query_import_targets;
#[path = "code_query_line_ranges.rs"]
mod code_query_line_ranges;
#[path = "code_query_path_ranking.rs"]
mod code_query_path_ranking;
#[path = "code_query_rows.rs"]
mod code_query_rows;
#[path = "code_query_scope.rs"]
mod code_query_scope;
#[path = "code_query_support.rs"]
mod code_query_support;

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

#[cfg(test)]
const MAX_CANDIDATE_BIND_VALUES: usize = 900;

use super::code_status::{repository_scope_status, repository_status};
use code_query_call_counts::{caller_target_call_counts, caller_target_call_key};
use code_query_import_targets::search_imports_by_target_symbols;
#[cfg(test)]
use code_query_import_targets::target_symbol_import_query;
use code_query_line_ranges::{
    SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, call_result_line_range,
    optional_line_range_with_symbol_context, symbol_result_line_range,
};
use code_query_path_ranking::{
    call_site_source_path_bonus, call_site_test_path_penalty, declaration_surface_path_bonus,
    query_mentions_test_or_benchmark, symbol_test_path_penalty,
};
use code_query_rows::{CallRow, ChunkRow, ImportRow, ReferenceRow, SymbolRow};
#[cfg(test)]
use code_query_scope::path_matches_filter;
use code_query_scope::selector_filters_fit_indexed_scope;
pub(super) use code_query_scope::{language_filter_allows, path_filter_allows};
use code_query_support::*;

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
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
        hits.extend(search_symbols(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::References
    ) {
        hits.extend(search_references(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Callers | CodeQueryKind::Callees
    ) {
        hits.extend(search_calls(connection, &status, &request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Imports
    ) {
        hits.extend(search_imports(connection, &status, &request)?);
    }
    if matches!(request.code_query_kind, CodeQueryKind::Hybrid) {
        hits.extend(search_chunks(connection, &status, &request)?);
    }
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

fn search_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = symbol_fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name, kind,
               (
                   SELECT MIN(previous.line_start)
                   FROM code_repository_symbols previous
                   WHERE previous.source_scope = code_repository_symbols.source_scope
                     AND previous.path = code_repository_symbols.path
                     AND previous.line_end < code_repository_symbols.line_start
                     AND code_repository_symbols.line_start - previous.line_end <= {SYMBOL_CONTEXT_PREAMBLE_MAX_LINES}
               ) AS previous_symbol_context_start
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND symbol_snapshot_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'symbol'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
        )),
        |row| {
            Ok(SymbolRow {
                symbol_snapshot_id: row.get(0)?,
                canonical_symbol_id: row.get(1)?,
                file_id: row.get(2)?,
                path: row.get(3)?,
                language_id: row.get(4)?,
                signature: row.get(5)?,
                doc_comment: row.get(6)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(7)?,
                    end: row.get(8)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(9)?,
                    end: row.get(10)?,
                },
                name: row.get(11)?,
                qualified_name: row.get(12)?,
                kind: row.get(13)?,
                previous_symbol_context_start: row.get(14)?,
            })
        },
    )?;
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_query.score([
                row.name.as_str(),
                row.qualified_name.as_str(),
                row.kind.as_str(),
                row.signature.as_str(),
                row.doc_comment.as_deref().unwrap_or_default(),
                row.path.as_str(),
            ]) + symbol_query_bonus(
                query,
                &row.name,
                &row.qualified_name,
                &row.signature,
                &row.canonical_symbol_id,
                request,
            );
            (score > 0.0).then(|| {
                let score = score
                    + 2.0
                    + symbol_kind_bonus(&row.kind, request)
                    + symbol_test_path_penalty(score, &row.path, request, query_has_test_intent);
                let line_range = symbol_result_line_range(&row);
                let excerpt = symbol_excerpt(
                    &row.name,
                    &row.qualified_name,
                    &row.signature,
                    row.doc_comment.as_deref(),
                );
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range,
                        symbol_snapshot_id: Some(row.symbol_snapshot_id),
                        canonical_symbol_id: Some(row.canonical_symbol_id),
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![
                            CodeRetrievalLayer::Symbol,
                            CodeRetrievalLayer::Definition,
                        ],
                        score,
                        excerpt,
                        degraded_reason: None,
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
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
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
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language_and_call_direction(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
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
                caller_excerpt: row.get(18)?,
            })
        },
    )?;
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
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
                    score_query.score([caller_name, caller_canonical_id]),
                    scoped_identity_query_bonus(query, [caller_canonical_id]),
                ),
                CodeQueryKind::Callers => (
                    score_query.score([row.callee_name.as_str(), target_hint, callee_canonical_id]),
                    scoped_identity_query_bonus(query, [target_hint, callee_canonical_id]),
                ),
                _ => (
                    score_query.score([
                        caller_name,
                        row.callee_name.as_str(),
                        target_hint,
                        caller_canonical_id,
                        callee_canonical_id,
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
                + same_named_caller_penalty(row.caller_name.as_deref(), &row.callee_name, request)
                + repeated_site_bonus
                + callee_related_name_bonus(query, &row.callee_name, request);
            let score = score + source_path_bonus + test_path_penalty;
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
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
        )),
        |row| {
            Ok(ImportRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                module: row.get(3)?,
                matched_symbol_name: None,
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
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    rows.extend(search_imports_by_target_symbols(
        connection, status, request,
    )?);

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
                + import_line_priority(base_score, row.line_range.start)
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
                        excerpt: row.module,
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
               symbol.canonical_symbol_id, f.parse_status, f.degraded_reason
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
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
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
                parse_status: row.get(10)?,
                degraded_reason: row.get(11)?,
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
            let score = score_query.score([&row.content, &row.path])
                + declaration_bonus
                + declaration_surface_path_bonus(declaration_bonus, &row.path, request);
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

pub(super) fn required_repository(
    connection: &mut Connection,
    selector: &crate::domain::CodeRepositorySelector,
) -> Result<CodeRepositoryStatus, StorageError> {
    let status = repository_status(connection, &selector.repository)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' is not registered",
            selector.repository
        ))
    })?;
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let scoped_status = match repository_scope_status(
        connection,
        &selector.repository,
        &selector.ref_selector,
        &path_filters,
        &language_filters,
    )? {
        Some(status) => Some(status),
        None if (!selector.path_filters.is_empty() || !selector.language_filters.is_empty())
            && selector_filters_fit_indexed_scope(
                &status.path_filters,
                &status.language_filters,
                &selector.path_filters,
                &selector.language_filters,
            ) =>
        {
            repository_scope_status(
                connection,
                &selector.repository,
                &selector.ref_selector,
                &status.path_filters,
                &status.language_filters,
            )?
        }
        None => None,
    }
    .ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' has no index for ref {} and requested filters",
            selector.repository, selector.ref_selector
        ))
    })?;

    Ok(scoped_status)
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

fn selected_row(
    path: &str,
    language_id: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> bool {
    path_filter_allows(path, &status.path_filters)
        && path_filter_allows(path, &request.repository.path_filters)
        && language_filter_allows(language_id, &status.language_filters)
        && language_filter_allows(language_id, &request.repository.language_filters)
}

pub(super) fn chunk_layers(parse_status: &str) -> Vec<CodeRetrievalLayer> {
    let mut layers = vec![CodeRetrievalLayer::Lexical];
    if parse_status != "parsed" {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

pub(super) struct HitParts {
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) byte_range: RepositoryCodeRange,
    pub(super) line_range: RepositoryCodeRange,
    pub(super) symbol_snapshot_id: Option<String>,
    pub(super) canonical_symbol_id: Option<String>,
    pub(super) file_id: Option<String>,
    pub(super) retrieval_layers: Vec<CodeRetrievalLayer>,
    pub(super) score: f64,
    pub(super) excerpt: String,
    pub(super) degraded_reason: Option<String>,
    pub(super) edge_kind: Option<String>,
    pub(super) edge_resolution_state: Option<String>,
    pub(super) edge_target_hint: Option<String>,
    pub(super) edge_confidence_basis_points: Option<u16>,
    pub(super) edge_confidence_tier: Option<String>,
}

pub(super) fn hit_from_parts(status: &CodeRepositoryStatus, parts: HitParts) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: parts.path,
        language_id: parts.language_id,
        byte_range: parts.byte_range,
        line_range: parts.line_range,
        symbol_snapshot_id: parts.symbol_snapshot_id,
        canonical_symbol_id: parts.canonical_symbol_id,
        file_id: parts.file_id,
        retrieval_layers: parts.retrieval_layers,
        index_versions: vec![format!(
            "code:{}:{}",
            status
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped"),
            status.tree_hash.as_deref().unwrap_or("unindexed")
        )],
        stale: status.stale,
        degraded_reason: parts.degraded_reason,
        edge_kind: parts.edge_kind,
        edge_resolution_state: parts.edge_resolution_state,
        edge_target_hint: parts.edge_target_hint,
        edge_confidence_basis_points: parts.edge_confidence_basis_points,
        edge_confidence_tier: parts.edge_confidence_tier,
        score: parts.score,
        excerpt: parts.excerpt,
    }
}

pub(super) fn required_scope(status: &CodeRepositoryStatus) -> Result<&str, StorageError> {
    status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })
}

pub(super) fn dedupe_sort_truncate(hits: &mut Vec<CodeRetrievalHit>, limit: usize) {
    let mut best = BTreeMap::<(String, u32, String), CodeRetrievalHit>::new();
    for hit in hits.drain(..) {
        let key = (hit.path.clone(), hit.line_range.start, hit.excerpt.clone());
        match best.get(&key) {
            Some(existing) if existing.score >= hit.score => {
                let existing = best.get_mut(&key).expect("checked entry should exist");
                merge_hit_provenance(existing, &hit);
            }
            Some(_) => {
                let mut hit = hit;
                if let Some(existing) = best.get(&key) {
                    merge_hit_provenance(&mut hit, existing);
                }
                best.insert(key, hit);
            }
            _ => {
                best.insert(key, hit);
            }
        }
    }
    hits.extend(best.into_values());
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_range.start.cmp(&right.line_range.start))
    });
    hits.truncate(limit);
}

fn merge_hit_provenance(target: &mut CodeRetrievalHit, source: &CodeRetrievalHit) {
    for layer in &source.retrieval_layers {
        if !target.retrieval_layers.contains(layer) {
            target.retrieval_layers.push(*layer);
        }
    }
    for version in &source.index_versions {
        if !target.index_versions.contains(version) {
            target.index_versions.push(version.clone());
        }
    }
    if target.degraded_reason.is_none() {
        target.degraded_reason = source.degraded_reason.clone();
    }
    if target.symbol_snapshot_id.is_none() {
        target.symbol_snapshot_id = source.symbol_snapshot_id.clone();
    }
    if target.canonical_symbol_id.is_none() {
        target.canonical_symbol_id = source.canonical_symbol_id.clone();
    }
    if target.file_id.is_none() {
        target.file_id = source.file_id.clone();
    }
    if target.edge_kind.is_none() {
        target.edge_kind = source.edge_kind.clone();
        target.edge_resolution_state = source.edge_resolution_state.clone();
        target.edge_target_hint = source.edge_target_hint.clone();
        target.edge_confidence_basis_points = source.edge_confidence_basis_points;
        target.edge_confidence_tier = source.edge_confidence_tier.clone();
    }
}

#[cfg(test)]
#[path = "code_query_unit_tests.rs"]
mod tests;
