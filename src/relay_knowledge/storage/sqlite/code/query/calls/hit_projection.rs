use crate::domain::{
    CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
    CodeRetrievalRequest, RepositoryCodeRange,
};

use super::{
    super::{
        HitParts,
        excerpts::{call_excerpt, callee_excerpt},
        hit_from_parts,
        line_ranges::call_result_line_range,
        relevance::*,
        rows::CallRow,
        scoring::local_callable::local_callable_declaration_bonus,
        scoring::path_ranking::{
            CallSiteQueryIntent, call_site_example_path_penalty, call_site_source_path_bonus,
            call_site_test_path_penalty, callee_member_context_bonus,
            caller_result_assignment_bonus, query_mentions_example_or_sample,
            query_mentions_test_or_benchmark,
        },
        selected_row,
    },
    caller_context_scoring::caller_context_density_bonus,
    counts::{caller_target_call_counts, caller_target_call_key},
    display::{call_display_name, inferred_caller_name_from_excerpt},
    execution_order::{callee_execution_order, callee_execution_order_bonus},
    site_scoring::exact_caller_named_receiver_member_call_bonus,
    target_ranking::high_confidence_inferred_target_bonus,
};

pub(super) fn call_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<CallRow>,
) -> Vec<CodeRetrievalHit> {
    let query = request.query.as_str();
    let score_query = ScoreQuery::new(query);
    let query_has_test_intent = query_mentions_test_or_benchmark(query);
    let query_has_example_intent = query_mentions_example_or_sample(query);
    let call_site_query_intent = CallSiteQueryIntent {
        test_or_benchmark: query_has_test_intent,
        example_or_sample: query_has_example_intent,
    };
    let call_site_counts = (request.code_query_kind == CodeQueryKind::Callers)
        .then(|| caller_target_call_counts(&rows));
    let callee_execution_order = callee_execution_order(&rows, request);

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
                    score_query.score([
                        caller_name,
                        caller_canonical_id,
                        row.caller_signature.as_deref().unwrap_or_default(),
                    ]),
                    scoped_identity_query_bonus(query, [caller_canonical_id]),
                ),
                CodeQueryKind::Callers => (
                    score_query.score([
                        row.callee_name.as_str(),
                        target_hint,
                        callee_canonical_id,
                        row.callee_signature.as_deref().unwrap_or_default(),
                    ]),
                    scoped_identity_query_bonus(query, [target_hint, callee_canonical_id]),
                ),
                _ => (
                    score_query.score([
                        caller_name,
                        row.callee_name.as_str(),
                        target_hint,
                        caller_canonical_id,
                        callee_canonical_id,
                        row.caller_signature.as_deref().unwrap_or_default(),
                        row.callee_signature.as_deref().unwrap_or_default(),
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
            let example_path_penalty = call_site_example_path_penalty(
                base_score,
                &row.path,
                request,
                query_has_example_intent,
            );
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
                + callee_member_context_bonus(
                    base_score,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + exact_caller_named_receiver_member_call_bonus(
                    base_score,
                    query,
                    row.caller_name.as_deref(),
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + caller_result_assignment_bonus(
                    base_score,
                    &row.path,
                    query,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                    call_site_query_intent,
                )
                + high_confidence_inferred_target_bonus(
                    base_score,
                    query,
                    &row.callee_name,
                    target_hint,
                    &row.resolution_state,
                    row.confidence_basis_points,
                    request,
                )
                + same_named_caller_penalty(row.caller_name.as_deref(), &row.callee_name, request)
                + caller_context_density_bonus(
                    base_score,
                    query,
                    row.caller_name.as_deref(),
                    &row.callee_name,
                    &row.path,
                    row.caller_excerpt.as_deref(),
                    request,
                )
                + local_callable_declaration_bonus(
                    base_score,
                    row.caller_excerpt.as_deref(),
                    &row.callee_name,
                    request,
                )
                + callee_execution_order_bonus(&callee_execution_order, &row, request)
                + repeated_site_bonus
                + callee_related_name_bonus(query, &row.callee_name, request);
            let score = score + source_path_bonus + test_path_penalty + example_path_penalty;
            (score > 0.0).then(|| {
                let line_range = call_result_line_range(request.code_query_kind, &row);
                let caller = call_display_name(
                    row.caller_name.as_deref(),
                    row.caller_canonical_symbol_id.as_deref(),
                )
                .or_else(|| inferred_caller_name_from_excerpt(row.caller_excerpt.as_deref()))
                .unwrap_or_else(|| "<module>".to_owned());
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
                        is_generated: row.is_generated,
                        excerpt: if request.code_query_kind == CodeQueryKind::Callees {
                            callee_excerpt(
                                row.caller_excerpt.as_deref(),
                                row.callee_excerpt.as_deref(),
                                &caller,
                                &row.callee_name,
                            )
                        } else {
                            call_excerpt(row.caller_excerpt.as_deref(), &caller, &row.callee_name)
                        },
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
        .collect()
}

#[cfg(test)]
#[path = "hit_projection_tests.rs"]
mod tests;
