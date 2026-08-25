use crate::domain::{
    CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
};

use super::typed_function_value::{TypedFunctionValueQuery, typed_function_value_surface_bonus};
use crate::storage::sqlite::code::query::{
    HitParts,
    api_identities::{ApiSymbolIdentity, api_identity_symbol_bonus},
    hit_from_parts,
    line_ranges::symbol_result_line_range,
    relevance::*,
    rows::SymbolRow,
    scoring::path_ranking::{
        path_looks_like_test_double, path_looks_like_test_or_benchmark, query_mentions_test_double,
        query_mentions_test_or_benchmark, symbol_declaration_surface_path_bonus,
        symbol_implementation_path_bonus, symbol_test_path_penalty,
    },
    selected_row,
};

pub(super) fn symbol_rows_to_hits(
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
        .filter(|row| {
            selected_row(
                &row.path,
                &row.language_id,
                row.is_generated,
                status,
                request,
            )
        })
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
                + scoped_member_identity_bonus(exact_identity.as_ref(), &row, request)
                + type_symbol_identity_bonus(exact_identity.as_ref(), &row, request)
                + hybrid_exact_type_role_bonus(exact_identity.as_ref(), &row, request)
                + typed_function_value_surface_bonus(
                    &row,
                    typed_function_value_query.as_ref(),
                    query_has_test_intent,
                )
                + if query_has_test_intent || !path_looks_like_test_or_benchmark(&row.path) {
                    hybrid_type_documentation_surface_bonus(
                        query,
                        &row.kind,
                        &row.name,
                        &row.signature,
                        row.doc_comment.as_deref(),
                        request,
                    )
                } else {
                    0.0
                };
            (score > 0.0).then(|| {
                let score = score
                    + 2.0
                    + symbol_kind_bonus(&row.kind, request)
                    + symbol_declaration_surface_path_bonus(score, &row.kind, &row.path, request)
                    + symbol_implementation_path_bonus(score, &row.signature, &row.path, request)
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
                        is_generated: row.is_generated,
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

fn hybrid_exact_type_role_bonus(
    identity: Option<&SymbolIdentityQuery>,
    row: &SymbolRow,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || !query_is_single_symbol_identity(&request.query)
        || !type_symbol_kind(&row.kind)
    {
        return 0.0;
    }
    let Some(identity) = identity else {
        return 0.0;
    };
    if !identity.matches_symbol(
        &row.name,
        &row.qualified_name,
        &row.signature,
        &row.canonical_symbol_id,
    ) {
        return 0.0;
    }

    let declaration_role_bonus = match row.kind.as_str() {
        "class" | "record" | "struct" => 0.8,
        "interface" | "trait" => 0.15,
        _ => 0.35,
    };
    let test_double_penalty =
        if path_looks_like_test_double(&row.path) && !query_mentions_test_double(&request.query) {
            -1.0
        } else {
            0.0
        };

    declaration_role_bonus + test_double_penalty
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
            selected_row(
                &row.path,
                &row.language_id,
                row.is_generated,
                status,
                request,
            ) && !path_looks_like_test_or_benchmark(&row.path)
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

fn scoped_member_identity_bonus(
    identity: Option<&SymbolIdentityQuery>,
    row: &SymbolRow,
    request: &CodeRetrievalRequest,
) -> f64 {
    if !matches!(
        request.code_query_kind,
        CodeQueryKind::Definition | CodeQueryKind::Symbol
    ) || type_symbol_kind(&row.kind)
    {
        return 0.0;
    }
    let Some(identity) = identity.filter(|identity| identity.is_scoped()) else {
        return 0.0;
    };
    if identity.matches_symbol(
        &row.name,
        &row.qualified_name,
        &row.signature,
        &row.canonical_symbol_id,
    ) {
        2.25
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "ranking_tests.rs"]
mod tests;
