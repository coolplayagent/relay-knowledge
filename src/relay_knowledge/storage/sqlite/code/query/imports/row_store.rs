//! Bounded direct, identifier, and FTS import-row retrieval.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use rusqlite::{Connection, ErrorCode, Row, params_from_iter, types::Value};

use crate::storage::sqlite::code::search::EXACT_SEARCH_OWNER_PREDICATE_SQL;
use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::super::{prepare_code_search_statement, relevance::*, required_scope, rows::ImportRow};
use super::path_context::{import_path_lookup_token, import_target_symbol_query};

pub(super) struct ImportPathRows {
    pub(super) rows: Vec<ImportRow>,
    saturated: bool,
}

const IMPORT_IDENTIFIER_SQL_PROGRESS_INTERVAL: i32 = 1_000;
const MAX_IMPORT_IDENTIFIER_SQL_PROGRESS_CALLBACKS: usize = 4_096;

struct ImportIdentifierProbe {
    rows: Vec<ImportRow>,
    saturated: bool,
}

struct ImportIdentifierProbeBudget {
    progress_interval: i32,
    max_progress_callbacks: usize,
}

pub(super) fn search_import_identifier_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportRow>, StorageError> {
    let probe = search_import_identifier_rows_with_progress_budget(
        connection,
        status,
        request,
        ImportIdentifierProbeBudget {
            progress_interval: IMPORT_IDENTIFIER_SQL_PROGRESS_INTERVAL,
            max_progress_callbacks: MAX_IMPORT_IDENTIFIER_SQL_PROGRESS_CALLBACKS,
        },
    )?;
    debug_assert!(!probe.saturated || probe.rows.is_empty());
    Ok(probe.rows)
}

fn search_import_identifier_rows_with_progress_budget(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    budget: ImportIdentifierProbeBudget,
) -> Result<ImportIdentifierProbe, StorageError> {
    let patterns = import_identifier_patterns(&request.query);
    if request.code_query_kind != CodeQueryKind::Imports || patterns.is_empty() {
        return Ok(ImportIdentifierProbe {
            rows: Vec::new(),
            saturated: false,
        });
    }

    let progress_callbacks = Arc::new(AtomicUsize::new(0));
    let observed_callbacks = Arc::clone(&progress_callbacks);
    connection.progress_handler(
        budget.progress_interval,
        Some(move || {
            observed_callbacks.fetch_add(1, Ordering::Relaxed) >= budget.max_progress_callbacks
        }),
    );
    let result = search_import_identifier_rows_with_active_progress_handler(
        connection, status, request, &patterns,
    );
    connection.progress_handler(0, None::<fn() -> bool>);

    match result {
        Ok(rows) => Ok(ImportIdentifierProbe {
            rows,
            saturated: false,
        }),
        Err(StorageError::Sqlite(error)) if sqlite_operation_interrupted(&error) => {
            Ok(ImportIdentifierProbe {
                rows: Vec::new(),
                saturated: true,
            })
        }
        Err(error) => Err(error),
    }
}

fn search_import_identifier_rows_with_active_progress_handler(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    patterns: &[String],
) -> Result<Vec<ImportRow>, StorageError> {
    let path_filter = path_filter_sql_for_column("i.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let predicates = patterns
        .iter()
        .map(|_| {
            "(lower(i.module) LIKE ? ESCAPE '\\' OR lower(coalesce(i.target_hint, '')) LIKE ? ESCAPE '\\')"
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated, f.line_count
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND ({predicates})
          {path_filter}
          {language_filter}
          {generated_filter}
        ORDER BY f.is_generated ASC, i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    for pattern in patterns {
        values.push(Value::Text(pattern.clone()));
        values.push(Value::Text(pattern.clone()));
    }
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    values.push(Value::Integer(
        candidate_limit(request, CandidateLayer::Import) as i64,
    ));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_import)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn sqlite_operation_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == ErrorCode::OperationInterrupted
    )
}

fn import_identifier_patterns(query: &str) -> Vec<String> {
    query_terms(query)
        .into_iter()
        .filter(|term| term.len() >= 3)
        .filter(|term| !import_identifier_stop_term(term))
        .take(8)
        .map(|term| format!("%{}%", escape_sql_like(&term.to_ascii_lowercase())))
        .collect()
}

fn import_identifier_stop_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "and"
            | "are"
            | "crate"
            | "extern"
            | "for"
            | "from"
            | "import"
            | "include"
            | "require"
            | "the"
            | "type"
            | "use"
            | "using"
    )
}

pub(super) fn search_import_path_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<ImportPathRows, StorageError> {
    let Some(pattern) = import_path_lookup_pattern(request) else {
        return Ok(ImportPathRows {
            rows: Vec::new(),
            saturated: false,
        });
    };
    let direct_limit = super::IMPORT_EXACT_EDGE_RESERVE_LIMIT;
    let path_filter = path_filter_sql_for_column("i.path", status, request);
    let mut query_path_clauses = Vec::new();
    push_query_path_substring_filter_sql(
        &mut query_path_clauses,
        "i.path",
        &request.query_path_substrings,
    );
    let query_path_filter = if query_path_clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", query_path_clauses.join(" AND "))
    };
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated, f.line_count
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND (
              lower(i.module) LIKE ? ESCAPE '\\'
              OR lower(coalesce(i.target_hint, '')) LIKE ? ESCAPE '\\'
          )
          {path_filter}
          {query_path_filter}
          {language_filter}
          {generated_filter}
        ORDER BY f.is_generated ASC, i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        Value::Text(required_scope(status)?.to_owned()),
        Value::Text(pattern.clone()),
        Value::Text(pattern),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_query_path_substring_filter_values(&mut values, &request.query_path_substrings);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    values.push(Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_import)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(ImportPathRows { rows, saturated })
}

fn import_path_lookup_pattern(request: &CodeRetrievalRequest) -> Option<String> {
    let path_token = import_path_lookup_token(&request.query)?;

    Some(format!(
        "%{}%",
        escape_sql_like(&path_token.to_ascii_lowercase())
    ))
}

pub(super) fn import_path_rows_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    rows: &ImportPathRows,
) -> bool {
    request.code_query_kind == CodeQueryKind::Imports
        && !rows.rows.is_empty()
        && (!rows.saturated || rows.rows.len() >= request.limit.max(1))
}

pub(super) fn import_path_rows_fit_request(
    request: &CodeRetrievalRequest,
    rows: &ImportPathRows,
) -> bool {
    !rows.saturated && rows.rows.len() <= request.limit.max(1)
}

pub(super) fn import_target_symbol_rows_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    rows: &[ImportRow],
) -> bool {
    request.code_query_kind == CodeQueryKind::Imports
        && import_target_symbol_query(&request.query).is_some()
        && !rows.is_empty()
        && rows.len() <= request.limit.max(1)
}

pub(super) fn search_import_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated, f.line_count
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
                {EXACT_SEARCH_OWNER_PREDICATE_SQL}
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (
                    SELECT 1 FROM code_repository_files fts_file
                    WHERE fts_file.source_scope = code_repository_search.source_scope
                      AND fts_file.path = code_repository_search.path
                      AND fts_file.is_generated != 0
                ))
              ORDER BY coalesce((
                    SELECT fts_file.is_generated FROM code_repository_files fts_file
                    WHERE fts_file.source_scope = code_repository_search.source_scope
                      AND fts_file.path = code_repository_search.path
                    LIMIT 1
                  ), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
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
        row_to_import,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn row_to_import(row: &Row<'_>) -> rusqlite::Result<ImportRow> {
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
        is_generated: row.get::<_, i64>(10)? != 0,
        source_line_count: row.get(11)?,
    })
}

#[cfg(test)]
#[path = "row_store_tests.rs"]
mod tests;
