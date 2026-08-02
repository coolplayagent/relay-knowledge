use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::{
    super::{
        line_ranges::optional_line_range_with_symbol_context, prepare_code_search_statement,
        relevance::*, required_scope, rows::CallRow,
    },
    direction::{
        call_direction_fts_filter_sql, fts_values_for_limited_with_language_and_call_direction,
    },
    identity_query::{CallIdentityQuery, call_identity_candidate_limit},
};

pub(super) struct CallIdentityRows {
    pub(super) rows: Vec<CallRow>,
    pub(super) saturated: bool,
}

pub(super) fn search_call_identity_rows(
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
    push_language_filter_values(&mut values, &request.query_language_filters);
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

pub(super) fn search_call_fts_rows(
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

pub(super) fn call_rows_sql(predicate_sql: &str) -> String {
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

pub(super) fn row_to_call(row: &Row<'_>) -> rusqlite::Result<CallRow> {
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

#[cfg(test)]
#[path = "row_store_tests.rs"]
mod tests;
