use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest},
    storage::StorageError,
};

use super::row_mapping::row_to_symbol;
use crate::storage::sqlite::code::query::{
    hybrid::exact_path::request_has_exact_file_filter,
    line_ranges::SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, prepare_code_search_statement, relevance::*,
    required_scope, rows::SymbolRow,
};

pub(super) fn search_symbol_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<SymbolRow>, StorageError> {
    let fts_query = symbol_fts_match_query_for_request(request);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let kind_filter = kind_filter_sql_for_column("code_repository_symbols.kind", request);
    let inner_kind_filter = kind_filter_sql_for_column("search_symbol.kind", request);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name, kind,
               coalesce((
                   SELECT file.is_generated
                   FROM code_repository_files file
                   WHERE file.source_scope = code_repository_symbols.source_scope
                     AND file.path = code_repository_symbols.path
                   LIMIT 1
               ), 0) AS is_generated,
               CASE WHEN code_repository_symbols.kind = 'class' THEN (
                   SELECT MIN(previous.line_start)
                   FROM code_repository_symbols previous
                   WHERE previous.source_scope = code_repository_symbols.source_scope
                     AND previous.path = code_repository_symbols.path
                     AND previous.line_end < code_repository_symbols.line_start
                     AND code_repository_symbols.line_start - previous.line_end <= {SYMBOL_CONTEXT_PREAMBLE_MAX_LINES}
               ) ELSE NULL END AS previous_symbol_context_start
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND symbol_snapshot_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'symbol'
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (SELECT 1 FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path AND fts_file.is_generated != 0))
                AND (
                    NOT EXISTS (
                        SELECT 1
                        FROM code_repository_symbols search_symbol
                        WHERE search_symbol.source_scope = code_repository_search.source_scope
                          AND search_symbol.symbol_snapshot_id = code_repository_search.record_id
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM code_repository_symbols search_symbol
                        WHERE search_symbol.source_scope = code_repository_search.source_scope
                          AND search_symbol.symbol_snapshot_id = code_repository_search.record_id
                          {inner_kind_filter}
                    )
                )
              ORDER BY coalesce((SELECT fts_file.is_generated FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path LIMIT 1), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
              LIMIT ?
          )
          {kind_filter}
        ORDER BY is_generated ASC, path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let mut values = fts_values_for_limited_with_language(
        required_scope(status)?,
        status,
        request,
        &fts_query,
        candidate_limit(request, CandidateLayer::Symbol),
        candidate_limit(request, CandidateLayer::Symbol),
    );
    let limit = values
        .pop()
        .expect("symbol fts values should include the outer limit");
    let fts_limit = values
        .pop()
        .expect("symbol fts values should include the fts limit");
    push_kind_filter_values(&mut values, request);
    values.push(fts_limit);
    push_kind_filter_values(&mut values, request);
    values.push(limit);
    let rows = statement.query_map(params_from_iter(values), row_to_symbol)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn symbol_fts_match_query_for_request(request: &CodeRetrievalRequest) -> String {
    if (request.code_query_kind == CodeQueryKind::Hybrid || request_has_exact_file_filter(request))
        && let Some(query) = focused_symbol_fts_match_query(&request.query)
    {
        return query;
    }

    symbol_fts_match_query(&request.query)
}

#[cfg(test)]
#[path = "fts_tests.rs"]
mod tests;
