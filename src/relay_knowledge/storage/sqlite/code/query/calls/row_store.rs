use rusqlite::{Connection, Row, params_from_iter, types::Value};

use super::row_budget;

use crate::storage::sqlite::code::search::EXACT_SEARCH_OWNER_PREDICATE_SQL;
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
    row_budget::run(connection, row_budget::MAX_PROGRESS_CALLBACKS, || {
        search_call_identity_rows_with_budget(connection, status, request, identity)
    })
}

fn search_call_identity_rows_with_budget(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &CallIdentityQuery,
) -> Result<CallIdentityRows, StorageError> {
    if identity.canonical_id.is_some() || identity.snapshot_id.is_some() {
        crate::storage::sqlite::code::schema::require_canonical_call_query_indexes(connection)?;
    }
    let exact_snapshot_id = if let Some(canonical_id) = &identity.canonical_id {
        let first = canonical_callable_snapshot(connection, required_scope(status)?, canonical_id)?;
        let Some(snapshot_id) = first else {
            return Ok(CallIdentityRows {
                rows: Vec::new(),
                saturated: false,
            });
        };
        Some(snapshot_id)
    } else {
        identity.snapshot_id.clone()
    };
    let path_filter = path_filter_sql_for_column("c.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let mut inline_filters = Vec::new();
    push_query_path_substring_filter_sql(
        &mut inline_filters,
        "c.path",
        &request.query_path_substrings,
    );
    let result_identity_column = match request.code_query_kind {
        crate::domain::CodeQueryKind::Callees => "callee.canonical_symbol_id",
        _ => "caller.canonical_symbol_id",
    };
    push_query_path_substring_filter_sql(
        &mut inline_filters,
        result_identity_column,
        &request.query_name_substrings,
    );
    let inline_filters = if inline_filters.is_empty() {
        String::new()
    } else {
        format!("AND {}", inline_filters.join(" AND "))
    };
    let direct_limit = call_identity_candidate_limit(request);
    let predicate = format!(
        "
          AND {} = ?
          {path_filter}
          {inline_filters}
          {language_filter}
        ",
        identity.match_column()
    );
    let mut values = vec![
        Value::Text(required_scope(status)?.to_owned()),
        Value::Text(
            exact_snapshot_id
                .as_deref()
                .unwrap_or_else(|| identity.leaf_name())
                .to_owned(),
        ),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_query_path_substring_filter_values(&mut values, &request.query_path_substrings);
    push_query_path_substring_filter_values(&mut values, &request.query_name_substrings);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    let mut rows = Vec::new();
    for generated in [false, true] {
        if rows.len() > direct_limit || (generated && request.exclude_generated) {
            break;
        }
        let generated_predicate = if generated { "!= 0" } else { "= 0" };
        let sql = ordered_call_rows_sql(
            &format!("{predicate} AND f.is_generated {generated_predicate}"),
            "c.path ASC, c.line_start ASC",
        );
        let mut page_values = values.clone();
        page_values.push(Value::Integer((direct_limit + 1 - rows.len()) as i64));
        let mut statement = prepare_code_search_statement(connection, &sql)?;
        let page = statement.query_map(params_from_iter(page_values), row_to_call)?;
        rows.extend(page.collect::<Result<Vec<_>, _>>()?);
    }
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(CallIdentityRows { rows, saturated })
}

// Bound canonical collisions without allowing declarations to hide later definitions.
const MAX_CANONICAL_SYMBOL_CANDIDATES: usize = 1024;

fn canonical_callable_snapshot(
    connection: &Connection,
    scope: &str,
    canonical_id: &str,
) -> Result<Option<String>, StorageError> {
    let mut statement = prepare_code_search_statement(
        connection,
        "SELECT symbol_snapshot_id, kind, signature FROM code_repository_symbols
         WHERE source_scope = ?1 AND canonical_symbol_id = ?2 LIMIT ?3",
    )?;
    let mut symbols = statement.query(rusqlite::params![
        scope,
        canonical_id,
        (MAX_CANONICAL_SYMBOL_CANDIDATES + 1) as i64,
    ])?;
    let mut selected = None;
    let mut declaration = None;
    let mut multiple_declarations = false;
    let mut count = 0;
    while let Some(row) = symbols.next()? {
        count += 1;
        if count > MAX_CANONICAL_SYMBOL_CANDIDATES {
            return Err(StorageError::AmbiguousCodeSymbol(
                "canonical symbol exceeds the 1024-candidate definition budget; use the desired definition's symbol_snapshot_id (symbol:...) as --query".to_owned(),
            ));
        }
        let kind: String = row.get(1)?;
        let signature: String = row.get(2)?;
        if !crate::domain::code_call_targets::callable_definition_symbol(&kind, &signature) {
            if crate::domain::code_call_targets::callable_target_symbol_kind(&kind) {
                if declaration.is_some() {
                    multiple_declarations = true;
                } else {
                    declaration = Some(row.get::<_, String>(0)?);
                }
            }
            continue;
        }
        if selected.is_some() {
            return Err(StorageError::AmbiguousCodeSymbol(
                "canonical symbol matches multiple definitions in this scope; use the desired definition's symbol_snapshot_id (symbol:...) as --query".to_owned(),
            ));
        }
        selected = Some(row.get::<_, String>(0)?);
    }
    if selected.is_none() && multiple_declarations {
        return Err(StorageError::AmbiguousCodeSymbol(
            "canonical symbol matches multiple callable declarations without a definition; use the desired declaration's symbol_snapshot_id (symbol:...) as --query".to_owned(),
        ));
    }
    Ok(selected.or(declaration))
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
                {EXACT_SEARCH_OWNER_PREDICATE_SQL}
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
    ordered_call_rows_sql(
        predicate_sql,
        "f.is_generated ASC, c.path ASC, c.line_start ASC",
    )
}

fn ordered_call_rows_sql(predicate_sql: &str, ordering: &str) -> String {
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
        ORDER BY {ordering}
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

#[cfg(test)]
#[path = "row_work_budget_tests.rs"]
mod work_budget_tests;
