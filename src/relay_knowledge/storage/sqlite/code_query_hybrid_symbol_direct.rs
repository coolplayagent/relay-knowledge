use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
    storage::StorageError,
};

use super::{row_to_symbol, symbol_rows_to_hits};
use crate::storage::sqlite::code::{
    code_query::code_query_api_identities::ApiSymbolIdentity,
    code_query::code_query_hybrid_planning::{
        hybrid_query_prefers_chunk_first, hybrid_sequence_terms,
    },
    code_query::code_query_rows::SymbolRow,
    code_query::code_query_support::*,
    code_query::{dedupe_sort_truncate, prepare_code_search_statement, required_scope},
};

pub(super) fn search_hybrid_direct_symbol_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    api_identities: &[ApiSymbolIdentity],
) -> Result<Option<Vec<CodeRetrievalHit>>, StorageError> {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || !hybrid_query_prefers_chunk_first(request)
    {
        return Ok(None);
    }
    let rows = search_hybrid_direct_symbol_rows(connection, status, request)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut hits = symbol_rows_to_hits(status, request, rows, api_identities);
    dedupe_sort_truncate(&mut hits, request.limit);
    if hybrid_direct_symbol_hits_can_answer_without_fts(request, &hits) {
        Ok(Some(hits))
    } else {
        Ok(None)
    }
}

fn search_hybrid_direct_symbol_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<SymbolRow>, StorageError> {
    let patterns = candidate_patterns(&request.query, 8);
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let mut values = vec![rusqlite::types::Value::Text(
        required_scope(status)?.to_owned(),
    )];
    let candidate_filter = direct_symbol_candidate_filter(&patterns, &mut values);
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    let limit = request.limit.saturating_mul(8).clamp(24, 96);
    values.push(rusqlite::types::Value::Integer(limit as i64));
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name, kind,
               NULL AS previous_symbol_context_start
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND ({candidate_filter})
          {path_filter}
          {language_filter}
        ORDER BY path ASC,
                 line_start ASC,
                 CASE kind
                     WHEN 'function' THEN 0
                     WHEN 'method' THEN 1
                     WHEN 'class' THEN 2
                     WHEN 'interface' THEN 3
                     WHEN 'struct' THEN 4
                     WHEN 'enum' THEN 5
                     ELSE 6
                 END ASC,
                 name ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_symbol)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn direct_symbol_candidate_filter(
    patterns: &[String],
    values: &mut Vec<rusqlite::types::Value>,
) -> String {
    patterns
        .iter()
        .map(|pattern| {
            [
                "lower(name)",
                "lower(qualified_name)",
                "lower(signature)",
                "lower(path)",
            ]
            .iter()
            .map(|field| {
                values.push(rusqlite::types::Value::Text(pattern.clone()));
                format!("{field} LIKE ? ESCAPE '\\'")
            })
            .collect::<Vec<_>>()
            .join(" OR ")
        })
        .map(|group| format!("({group})"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn hybrid_direct_symbol_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 5 {
        return false;
    }
    let required_coverage = terms.len().saturating_mul(2).div_ceil(3).max(4);
    let mut covered_terms = Vec::new();
    let mut supporting_hits = 0usize;
    for hit in hits.iter().take(request.limit.max(1)) {
        if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            || !hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::Definition)
        {
            continue;
        }
        let surface = format!(
            "{} {} {}",
            hit.excerpt.to_ascii_lowercase(),
            hit.canonical_symbol_id
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            hit.path.to_ascii_lowercase()
        );
        let mut matched = 0usize;
        for term in &terms {
            if surface.contains(term.as_str()) {
                matched += 1;
                if !covered_terms.contains(term) {
                    covered_terms.push(term.clone());
                }
            }
        }
        if matched >= 2 && hit.score >= 4.0 {
            supporting_hits += 1;
        }
    }

    supporting_hits >= 2 && covered_terms.len() >= required_coverage
}
