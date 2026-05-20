use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts,
    code_query_line_ranges::{SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, symbol_result_line_range},
    code_query_path_ranking::{
        query_mentions_test_or_benchmark, symbol_declaration_surface_path_bonus,
        symbol_test_path_penalty,
    },
    code_query_rows::SymbolRow,
    code_query_support::*,
    dedupe_sort_truncate, hit_from_parts, prepare_code_search_statement, required_scope,
    selected_row,
};

struct SymbolIdentityRows {
    rows: Vec<SymbolRow>,
    saturated: bool,
}

pub(super) fn search_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        let identity_rows = search_symbol_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| {
                identity.matches_symbol(
                    &row.name,
                    &row.qualified_name,
                    &row.signature,
                    &row.canonical_symbol_id,
                )
            })
            .collect::<Vec<_>>();
        identity_hits = symbol_rows_to_hits(status, request, rows);
        if identity_hits_can_answer_without_fts(request, identity, identity_hits.len(), saturated) {
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    }

    let mut hits = symbol_rows_to_hits(
        status,
        request,
        search_symbol_fts_rows(connection, status, request)?,
    );
    hits.extend(identity_hits);

    Ok(hits)
}

fn search_symbol_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
) -> Result<SymbolIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let direct_limit = symbol_identity_candidate_limit(request);
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
          AND name = ?
          {path_filter}
          {language_filter}
        ORDER BY path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        rusqlite::types::Value::Text(required_scope(status)?.to_owned()),
        rusqlite::types::Value::Text(identity.leaf_name().to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(rusqlite::types::Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_symbol)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(SymbolIdentityRows { rows, saturated })
}

fn search_symbol_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<SymbolRow>, StorageError> {
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
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Symbol),
            candidate_limit(request, CandidateLayer::Symbol),
        )),
        row_to_symbol,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn row_to_symbol(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRow> {
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
}

fn symbol_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<SymbolRow>,
) -> Vec<CodeRetrievalHit> {
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);

    rows.into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_query.score([
                row.name.as_str(),
                row.qualified_name.as_str(),
                row.kind.as_str(),
                row.signature.as_str(),
                row.doc_comment.as_deref().unwrap_or_default(),
                row.path.as_str(),
            ]) + score_exact_path(query, &row.path)
                + symbol_query_bonus(
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
                    + symbol_declaration_surface_path_bonus(score, &row.kind, &row.path, request)
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
        .collect()
}

fn identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Definition | CodeQueryKind::Symbol
        )
        && (identity.is_scoped() || hit_count <= request.limit)
}

fn symbol_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Symbol).min(200)
}
