use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
    storage::StorageError,
};

use super::row_mapping::row_to_symbol;
use crate::storage::sqlite::code::query::{
    api_identities::ApiSymbolIdentity, canonical_symbol_leaf_matches,
    line_ranges::SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, prepare_code_search_statement, relevance::*,
    required_scope, rows::SymbolRow,
};

pub(super) struct SymbolIdentityRows {
    pub(super) rows: Vec<SymbolRow>,
    pub(super) saturated: bool,
}

pub(super) struct ApiIdentityRows {
    pub(super) rows: Vec<SymbolRow>,
    pub(super) matched_identity_count: usize,
    pub(super) saturated: bool,
}

pub(in crate::storage::sqlite::code::query) fn hybrid_symbol_query_can_answer_without_non_symbol_layers(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || hits.is_empty()
        || !query_is_single_symbol_identity(&request.query)
    {
        return false;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return false;
    };

    let exact_symbol_hits = hits
        .iter()
        .filter(|hit| hybrid_symbol_hit_matches_identity(hit, &identity))
        .count();
    exact_symbol_hits > 0 && exact_symbol_hits <= request.limit.max(1)
}

fn hybrid_symbol_hit_matches_identity(
    hit: &CodeRetrievalHit,
    identity: &SymbolIdentityQuery,
) -> bool {
    if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
        || hit.symbol_snapshot_id.is_none()
    {
        return false;
    }
    let Some(canonical_symbol_id) = hit.canonical_symbol_id.as_deref() else {
        return false;
    };

    if identity.is_scoped() {
        identity.matches_symbol(
            identity.leaf_name(),
            &hit.excerpt,
            &hit.excerpt,
            canonical_symbol_id,
        )
    } else {
        canonical_symbol_leaf_matches(canonical_symbol_id, identity.leaf_name())
    }
}

pub(super) fn symbol_identity_name_exists(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    name: &str,
) -> Result<bool, StorageError> {
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let kind_filter = kind_filter_sql_for_column("kind", request);
    let sql = format!(
        "
        SELECT 1
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND name = ?
          {path_filter}
          {language_filter}
          {kind_filter}
        LIMIT 1
        "
    );
    let mut values = vec![
        rusqlite::types::Value::Text(required_scope(status)?.to_owned()),
        rusqlite::types::Value::Text(name.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    push_kind_filter_values(&mut values, request);

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let mut rows = statement.query(params_from_iter(values))?;

    rows.next()
        .map_err(StorageError::from)
        .map(|row| row.is_some())
}

pub(super) fn search_symbol_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
) -> Result<SymbolIdentityRows, StorageError> {
    let scoped_pattern = identity.scoped_like_pattern();
    search_symbol_identity_rows_by_name(
        connection,
        status,
        request,
        identity.leaf_name(),
        symbol_identity_candidate_limit(request),
        scoped_pattern.as_deref(),
    )
}

fn search_symbol_identity_rows_by_name(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    name: &str,
    direct_limit: usize,
    scoped_pattern: Option<&str>,
) -> Result<SymbolIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let kind_filter = kind_filter_sql_for_column("kind", request);
    let scoped_filter = if scoped_pattern.is_some() {
        "AND (
                   lower(qualified_name) LIKE ? ESCAPE '\\'
                OR lower(signature) LIKE ? ESCAPE '\\'
                OR lower(canonical_symbol_id) LIKE ? ESCAPE '\\'
            )"
    } else {
        ""
    };
    let generated_filter = if request.exclude_generated {
        "AND coalesce((
                   SELECT file.is_generated
                   FROM code_repository_files file
                   WHERE file.source_scope = code_repository_symbols.source_scope
                     AND file.path = code_repository_symbols.path
                   LIMIT 1
               ), 0) = 0"
    } else {
        ""
    };
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
          AND name = ?
          {scoped_filter}
          {generated_filter}
          {path_filter}
          {language_filter}
          {kind_filter}
        ORDER BY is_generated ASC, path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        rusqlite::types::Value::Text(required_scope(status)?.to_owned()),
        rusqlite::types::Value::Text(name.to_owned()),
    ];
    if let Some(pattern) = scoped_pattern {
        values.extend([
            rusqlite::types::Value::Text(pattern.to_owned()),
            rusqlite::types::Value::Text(pattern.to_owned()),
            rusqlite::types::Value::Text(pattern.to_owned()),
        ]);
    }
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    push_kind_filter_values(&mut values, request);
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

pub(super) fn search_hybrid_api_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identities: &[ApiSymbolIdentity],
) -> Result<ApiIdentityRows, StorageError> {
    if identities.is_empty() {
        return Ok(ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: 0,
            saturated: false,
        });
    }

    let mut rows = Vec::new();
    let mut matched_identity_count = 0;
    let mut saturated = false;
    for identity in identities {
        let identity_rows = search_symbol_identity_rows_by_name(
            connection,
            status,
            request,
            identity.leaf_name(),
            hybrid_api_identity_candidate_limit(request),
            None,
        )?;
        saturated |= identity_rows.saturated;
        let matched_rows = identity_rows
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
        if !matched_rows.is_empty() {
            matched_identity_count += 1;
        }
        rows.extend(matched_rows);
    }

    Ok(ApiIdentityRows {
        rows,
        matched_identity_count,
        saturated,
    })
}

pub(super) fn api_identity_rows_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identities: &[ApiSymbolIdentity],
    rows: &ApiIdentityRows,
) -> bool {
    if identities.len() < 2 || rows.saturated {
        return false;
    }

    match request.code_query_kind {
        CodeQueryKind::Symbol => {
            rows.matched_identity_count == identities.len()
                && api_identity_query_terms_are_closed(&request.query, identities)
        }
        CodeQueryKind::Hybrid => rows.matched_identity_count == identities.len(),
        _ => false,
    }
}

fn api_identity_query_terms_are_closed(query: &str, identities: &[ApiSymbolIdentity]) -> bool {
    query
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .all(|token| {
            identities
                .iter()
                .any(|identity| identity.matches_query_token(token))
        })
}

pub(super) fn identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && query_is_single_symbol_identity(&request.query)
        && (matches!(
            request.code_query_kind,
            CodeQueryKind::Definition | CodeQueryKind::Symbol
        ) || request.code_query_kind == CodeQueryKind::Hybrid)
        && (identity.is_scoped() || hit_count <= request.limit)
}

pub(super) fn identity_miss_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    saturated: bool,
    identity: &SymbolIdentityQuery,
) -> bool {
    !saturated
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Definition | CodeQueryKind::Symbol
        )
        && query_is_single_symbol_identity(&request.query)
        && identity_has_exact_case_intent(identity.leaf_name())
}

fn identity_has_exact_case_intent(name: &str) -> bool {
    name.chars()
        .any(|character| character.is_ascii_uppercase() || character == '_')
}

fn symbol_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Symbol).min(200)
}

fn hybrid_api_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    request.limit.clamp(10, 40)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
