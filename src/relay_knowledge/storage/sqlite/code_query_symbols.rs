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
    code_query_api_identities::{
        ApiSymbolIdentity, api_identity_symbol_bonus, hybrid_api_symbol_identities,
    },
    code_query_line_ranges::{SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, symbol_result_line_range},
    code_query_path_ranking::{
        path_looks_like_test_or_benchmark, query_mentions_test_or_benchmark,
        symbol_declaration_surface_path_bonus, symbol_test_path_penalty,
    },
    code_query_rows::SymbolRow,
    code_query_support::*,
    code_search_plannable_outage_reason, dedupe_sort_truncate, hit_from_parts, mark_hits_degraded,
    prepare_code_search_statement, required_scope, selected_row,
};

struct SymbolIdentityRows {
    rows: Vec<SymbolRow>,
    saturated: bool,
}

struct ApiIdentityRows {
    rows: Vec<SymbolRow>,
    matched_identity_count: usize,
    saturated: bool,
}

struct TypedFunctionValueSurface {
    name_terms: Vec<String>,
    declared_type_terms: Vec<String>,
    signature_terms: Vec<String>,
    exported: bool,
}

struct TypedFunctionValueQuery {
    terms: Vec<String>,
    mentions_surface_intent: bool,
}

const TYPED_FUNCTION_VALUE_MIN_QUERY_TERMS: usize = 4;
const TYPED_FUNCTION_VALUE_MIN_MATCHED_TERMS: usize = 3;
const TYPED_FUNCTION_VALUE_BASE_BONUS: f64 = 1.25;
const TYPED_FUNCTION_VALUE_EXPORTED_BONUS: f64 = 0.35;
const TYPED_FUNCTION_VALUE_MAX_BONUS: f64 = 2.35;

pub(super) fn search_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let api_identities = hybrid_api_symbol_identities(&request.query, request);
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
        identity_hits = symbol_rows_to_hits(status, request, rows, &api_identities);
        if identity_hits_can_answer_without_fts(request, identity, identity_hits.len(), saturated) {
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    }

    let api_identity_rows =
        search_hybrid_api_identity_rows(connection, status, request, &api_identities)?;
    if api_identity_rows_can_answer_without_fts(request, &api_identities, &api_identity_rows) {
        let mut hits =
            symbol_rows_to_hits(status, request, api_identity_rows.rows, &api_identities);
        dedupe_sort_truncate(&mut hits, request.limit);
        return Ok(hits);
    }

    let symbol_fts_rows = match search_symbol_fts_rows(connection, status, request) {
        Ok(rows) => rows,
        Err(error) => {
            let Some(reason) = code_search_plannable_outage_reason(request, &error) else {
                return Err(error);
            };
            let mut hits = identity_hits;
            hits.extend(symbol_rows_to_hits(
                status,
                request,
                api_identity_rows.rows,
                &api_identities,
            ));
            if hits.is_empty() {
                return Err(error);
            }
            mark_hits_degraded(&mut hits, &reason);
            dedupe_sort_truncate(&mut hits, request.limit);
            return Ok(hits);
        }
    };
    let mut hits = symbol_rows_to_hits(status, request, symbol_fts_rows, &api_identities);
    hits.extend(identity_hits);
    hits.extend(symbol_rows_to_hits(
        status,
        request,
        api_identity_rows.rows,
        &api_identities,
    ));

    Ok(hits)
}

fn search_symbol_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
) -> Result<SymbolIdentityRows, StorageError> {
    search_symbol_identity_rows_by_name(
        connection,
        status,
        request,
        identity.leaf_name(),
        symbol_identity_candidate_limit(request),
    )
}

fn search_symbol_identity_rows_by_name(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    name: &str,
    direct_limit: usize,
) -> Result<SymbolIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name, kind,
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
          {path_filter}
          {language_filter}
        ORDER BY path ASC, line_start ASC
        LIMIT ?
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

fn search_hybrid_api_identity_rows(
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

fn api_identity_rows_can_answer_without_fts(
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
    api_identities: &[ApiSymbolIdentity],
) -> Vec<CodeRetrievalHit> {
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let exact_identity = SymbolIdentityQuery::from_query(query);
    let typed_function_value_query = TypedFunctionValueQuery::from_request(query, request);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
    let drop_test_symbols = should_drop_test_symbols(status, request, &rows, query_has_test_intent);

    rows.into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter(|row| !drop_test_symbols || !path_looks_like_test_or_benchmark(&row.path))
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
                )
                + api_identity_symbol_bonus(
                    api_identities,
                    &row.name,
                    &row.qualified_name,
                    &row.signature,
                    &row.canonical_symbol_id,
                )
                + type_symbol_identity_bonus(exact_identity.as_ref(), &row, request)
                + typed_function_value_surface_bonus(
                    &row,
                    typed_function_value_query.as_ref(),
                    query_has_test_intent,
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

fn should_drop_test_symbols(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: &[SymbolRow],
    query_has_test_intent: bool,
) -> bool {
    !query_has_test_intent
        && matches!(
            request.code_query_kind,
            CodeQueryKind::Definition | CodeQueryKind::Symbol
        )
        && rows.iter().any(|row| {
            selected_row(&row.path, &row.language_id, status, request)
                && !path_looks_like_test_or_benchmark(&row.path)
        })
}

fn type_symbol_identity_bonus(
    identity: Option<&SymbolIdentityQuery>,
    row: &SymbolRow,
    request: &CodeRetrievalRequest,
) -> f64 {
    if !matches!(
        request.code_query_kind,
        CodeQueryKind::Definition | CodeQueryKind::Symbol
    ) || !type_symbol_kind(&row.kind)
    {
        return 0.0;
    }
    let Some(identity) = identity else {
        return 0.0;
    };
    if identity.matches_symbol(
        &row.name,
        &row.qualified_name,
        &row.signature,
        &row.canonical_symbol_id,
    ) {
        0.55
    } else {
        0.0
    }
}

fn type_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class"
            | "enum"
            | "interface"
            | "record"
            | "struct"
            | "trait"
            | "type"
            | "type_alias"
            | "typedef"
            | "union"
    )
}

impl TypedFunctionValueQuery {
    fn from_request(query: &str, request: &CodeRetrievalRequest) -> Option<Self> {
        if request.code_query_kind != CodeQueryKind::Hybrid {
            return None;
        }
        let terms = symbol_surface_terms(query);
        if terms.len() < TYPED_FUNCTION_VALUE_MIN_QUERY_TERMS {
            return None;
        }

        Some(Self {
            mentions_surface_intent: query_mentions_typed_function_value(query, &terms),
            terms,
        })
    }
}

fn typed_function_value_surface_bonus(
    row: &SymbolRow,
    query: Option<&TypedFunctionValueQuery>,
    query_has_test_intent: bool,
) -> f64 {
    let Some(query) = query else {
        return 0.0;
    };
    if !matches!(row.kind.as_str(), "constant" | "function" | "variable")
        || (path_looks_like_test_or_benchmark(&row.path) && !query_has_test_intent)
    {
        return 0.0;
    }
    let Some(surface) = typed_function_value_surface(&row.signature) else {
        return 0.0;
    };
    if !terms_overlap(&query.terms, &surface.name_terms)
        || !terms_overlap(&query.terms, &surface.declared_type_terms)
    {
        return 0.0;
    }
    let matched_terms = query
        .terms
        .iter()
        .filter(|query_term| {
            surface
                .signature_terms
                .iter()
                .any(|surface_term| related_surface_terms(surface_term, query_term))
        })
        .count();
    if matched_terms < TYPED_FUNCTION_VALUE_MIN_MATCHED_TERMS
        || (!query.mentions_surface_intent
            && matched_terms < TYPED_FUNCTION_VALUE_MIN_MATCHED_TERMS + 1)
    {
        return 0.0;
    }

    let coverage = matched_terms as f64 / query.terms.len() as f64;
    (TYPED_FUNCTION_VALUE_BASE_BONUS
        + coverage
        + if surface.exported {
            TYPED_FUNCTION_VALUE_EXPORTED_BONUS
        } else {
            0.0
        })
    .min(TYPED_FUNCTION_VALUE_MAX_BONUS)
}

fn typed_function_value_surface(signature: &str) -> Option<TypedFunctionValueSurface> {
    let signature = signature.trim();
    if !(signature.contains("=>") && signature.contains('=')) {
        return None;
    }
    let (left_side, _) = signature.split_once('=')?;
    let (before_type, declared_type) = left_side.rsplit_once(':')?;
    let name = before_type
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())?;
    let name_terms = symbol_surface_terms(name);
    let declared_type_terms = symbol_surface_terms(declared_type);
    if name_terms.is_empty() || declared_type_terms.is_empty() {
        return None;
    }

    Some(TypedFunctionValueSurface {
        name_terms,
        declared_type_terms,
        signature_terms: symbol_surface_terms(signature),
        exported: signature.starts_with("export ") || signature.starts_with("pub "),
    })
}

fn query_mentions_typed_function_value(query: &str, query_terms: &[String]) -> bool {
    query.contains("=>")
        || query_terms.iter().any(|term| {
            matches!(
                term.as_str(),
                "arrow" | "callback" | "closure" | "function" | "inline" | "lambda" | "typed"
            )
        })
}

fn terms_overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| related_surface_terms(left, right)))
}

fn related_surface_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
}

fn symbol_surface_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
    {
        terms.push(token.to_ascii_lowercase());
        terms.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_surface_terms(token, &mut terms);
    }
    terms.sort();
    terms.dedup();

    terms
}

fn push_camel_surface_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0usize;
    for index in 1..chars.len() {
        let previous = chars[index - 1].1;
        let current = chars[index].1;
        let next = chars.get(index + 1).map(|(_, character)| *character);
        let starts_word = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
        let ends_acronym = previous.is_ascii_uppercase()
            && current.is_ascii_uppercase()
            && next.is_some_and(|character| character.is_ascii_lowercase());
        let changes_kind = previous.is_ascii_alphabetic() != current.is_ascii_alphabetic();
        if starts_word || ends_acronym || changes_kind {
            terms.push(token[start..chars[index].0].to_ascii_lowercase());
            start = chars[index].0;
        }
    }
    terms.push(token[start..].to_ascii_lowercase());
}

fn identity_hits_can_answer_without_fts(
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

fn symbol_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Symbol).min(200)
}

fn hybrid_api_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    request.limit.clamp(10, 40)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn api_dense_hybrid_query_skips_broad_symbol_fts_when_identities_cover() {
        let request = make_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        );
        let identities = hybrid_api_symbol_identities(&request.query, &request);
        let rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: identities.len(),
            saturated: false,
        };

        assert!(api_identity_rows_can_answer_without_fts(
            &request,
            &identities,
            &rows
        ));
    }

    #[test]
    fn api_dense_symbol_query_still_requires_closed_identity_terms() {
        let request = make_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Symbol,
        );
        let identities = hybrid_api_symbol_identities(&request.query, &request);
        let rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: identities.len(),
            saturated: false,
        };

        assert!(!api_identity_rows_can_answer_without_fts(
            &request,
            &identities,
            &rows
        ));

        let closed_request = make_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh",
            CodeQueryKind::Symbol,
        );
        let closed_identities =
            hybrid_api_symbol_identities(&closed_request.query, &closed_request);
        let closed_rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: closed_identities.len(),
            saturated: false,
        };

        assert!(api_identity_rows_can_answer_without_fts(
            &closed_request,
            &closed_identities,
            &closed_rows
        ));
    }

    #[test]
    fn api_dense_hybrid_query_keeps_broad_symbol_fts_for_partial_or_empty_identity_lookup() {
        let request = make_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        );
        let identities = hybrid_api_symbol_identities(&request.query, &request);
        let partial_rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: identities.len() - 1,
            saturated: false,
        };
        let empty_rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: 0,
            saturated: false,
        };
        let saturated_rows = ApiIdentityRows {
            rows: Vec::new(),
            matched_identity_count: identities.len(),
            saturated: true,
        };

        assert!(!api_identity_rows_can_answer_without_fts(
            &request,
            &identities,
            &partial_rows
        ));
        assert!(!api_identity_rows_can_answer_without_fts(
            &request,
            &identities,
            &empty_rows
        ));
        assert!(!api_identity_rows_can_answer_without_fts(
            &request,
            &identities,
            &saturated_rows
        ));
    }

    fn make_request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
        CodeRetrievalRequest::new(
            query,
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), vec!["go".to_owned()])
                .expect("selector should validate"),
            kind,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate")
    }
}
