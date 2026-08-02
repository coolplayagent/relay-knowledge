use std::sync::Arc;

use futures_util::{StreamExt, stream};

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositorySetQueryResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{
        CodeQueryKind, CodeRepositorySelector, CodeRepositorySetMemberStatus,
        CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest, CodeRepositorySetStatus,
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest, FreshnessPolicy,
    },
};

use super::super::super::source_fallback::apply_code_grep_fallback;
use super::super::{
    errors::storage_api_error, member_freshness::fact_version_scope_mismatch_reason,
    status::required_set_status,
};
use super::{
    OverlayEvidenceIndex, apply_bridge_support_bonus, dedupe_sort_truncate,
    per_member_candidate_limit,
    plan::{
        dependency_symbol_plan_needs_hybrid_fallback, merge_dependency_symbol_fallback_hits,
        repository_set_member_query_plan,
    },
    prune_returned_overlay_evidence, repository_set_score,
};

const REPOSITORY_SET_QUERY_MEMBER_CONCURRENCY: usize = 4;

impl RelayKnowledgeService {
    /// Queries every member scope and merges ranked candidates without changing single-repo search.
    pub async fn query_code_repository_set(
        &self,
        request: CodeRepositorySetQueryRequest,
        context: RequestContext,
    ) -> Result<CodeRepositorySetQueryResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_set_status(&store, &request.set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            return Ok(CodeRepositorySetQueryResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                request,
                status,
                results: Vec::new(),
                truncated: false,
                degraded_reason: Some("graph_only freshness policy selected".to_owned()),
            });
        }
        if let Some(error) = unfresh_set_error_for_wait_policy(&request, &status) {
            return Err(error);
        }
        let edges = store
            .code_repository_set_cross_edges(status.repository_set.set_id.clone())
            .await
            .map_err(storage_api_error)?;
        let edge_index = OverlayEvidenceIndex::new(&edges);
        let mut results = Vec::new();
        let candidate_limit = per_member_candidate_limit(request.limit, status.members.len());
        let highest_priority = status
            .members
            .iter()
            .map(|member| member.member.priority)
            .max()
            .unwrap_or(0);
        let member_outcomes = stream::iter(status.members.iter().cloned())
            .map(|member_status| {
                query_repository_set_member(
                    Arc::clone(&store),
                    request.clone(),
                    member_status,
                    highest_priority,
                    candidate_limit,
                )
            })
            .buffer_unordered(REPOSITORY_SET_QUERY_MEMBER_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        let mut outcomes = Vec::new();
        for outcome in member_outcomes {
            outcomes.push(outcome?);
        }
        results.extend(repository_set_results_from_outcomes(
            &request.query,
            &outcomes,
            &edge_index,
        ));
        apply_bridge_support_bonus(&mut results);
        if repository_set_deferred_source_fallback_needed(&request, &outcomes, &results) {
            apply_repository_set_deferred_source_fallbacks(
                Arc::clone(&store),
                &request,
                &mut outcomes,
            )
            .await?;
            results.clear();
            results.extend(repository_set_results_from_outcomes(
                &request.query,
                &outcomes,
                &edge_index,
            ));
            apply_bridge_support_bonus(&mut results);
        }
        let truncated = dedupe_sort_truncate(&mut results, request.limit, &request.query);
        prune_returned_overlay_evidence(&mut results);
        let mut degraded_reasons = vec![
            status.degraded_reason.clone(),
            status
                .overlay
                .stale
                .then(|| "repository set overlay is stale".to_owned()),
        ];
        degraded_reasons.extend(outcomes.into_iter().map(|outcome| outcome.degraded_reason));
        let degraded_reason = join_degraded_reasons(degraded_reasons);

        Ok(CodeRepositorySetQueryResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            request,
            status,
            results,
            truncated,
            degraded_reason,
        })
    }
}

struct RepositorySetMemberQueryOutcome {
    member_status: CodeRepositorySetMemberStatus,
    hits: Vec<CodeRetrievalHit>,
    active_request: CodeRetrievalRequest,
    dependency_symbol_plan_satisfied: bool,
    source_fallback_allowed: bool,
    degraded_reason: Option<String>,
}

struct RepositorySetMemberSourceFallbackInput {
    index: usize,
    member_status: CodeRepositorySetMemberStatus,
    active_request: CodeRetrievalRequest,
    hits: Vec<CodeRetrievalHit>,
}

struct RepositorySetMemberSourceFallbackOutput {
    index: usize,
    hits: Vec<CodeRetrievalHit>,
    degraded_reason: Option<String>,
}

async fn query_repository_set_member(
    store: Arc<dyn crate::storage::KnowledgeStore>,
    request: CodeRepositorySetQueryRequest,
    member_status: CodeRepositorySetMemberStatus,
    highest_priority: i32,
    candidate_limit: usize,
) -> Result<RepositorySetMemberQueryOutcome, ApiError> {
    let member = &member_status.member;
    let selector = CodeRepositorySelector::new(
        member.repository_alias.clone(),
        member.resolved_commit_sha.clone(),
        request.path_filters.clone(),
        request.language_filters.clone(),
    )
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let member_query_plan =
        repository_set_member_query_plan(&request, &member_status, highest_priority);
    let mut search_request = CodeRetrievalRequest::new(
        member_query_plan.query,
        selector.clone(),
        member_query_plan.kind,
        candidate_limit,
        FreshnessPolicy::AllowStale,
    )
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    search_request.exclude_generated = request.exclude_generated;
    let mut active_request = search_request.clone();
    if let Some(reason) = fact_version_scope_mismatch_reason(&member_status) {
        return Ok(RepositorySetMemberQueryOutcome {
            member_status,
            hits: Vec::new(),
            active_request,
            dependency_symbol_plan_satisfied: false,
            source_fallback_allowed: false,
            degraded_reason: Some(reason),
        });
    }
    let mut hits = store
        .search_code_scope(member.source_scope.clone(), search_request)
        .await
        .map_err(storage_api_error)?;
    let dependency_symbol_plan_needs_fallback =
        dependency_symbol_plan_needs_hybrid_fallback(&request, member_query_plan.kind, &hits);
    let dependency_symbol_plan_satisfied = request.code_query_kind == CodeQueryKind::Hybrid
        && member_query_plan.kind == CodeQueryKind::Symbol
        && !dependency_symbol_plan_needs_fallback;
    if dependency_symbol_plan_needs_fallback {
        let symbol_plan_hits = hits;
        let mut fallback_request = CodeRetrievalRequest::new(
            request.query.clone(),
            selector,
            request.code_query_kind,
            candidate_limit,
            FreshnessPolicy::AllowStale,
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        fallback_request.exclude_generated = request.exclude_generated;
        active_request = fallback_request.clone();
        let fallback_hits = store
            .search_code_scope(member.source_scope.clone(), fallback_request)
            .await
            .map_err(storage_api_error)?;
        hits = merge_dependency_symbol_fallback_hits(symbol_plan_hits, fallback_hits);
    }
    Ok(RepositorySetMemberQueryOutcome {
        member_status,
        hits,
        active_request,
        dependency_symbol_plan_satisfied,
        source_fallback_allowed: true,
        degraded_reason: None,
    })
}

fn repository_set_results_from_outcomes(
    query: &str,
    outcomes: &[RepositorySetMemberQueryOutcome],
    edge_index: &OverlayEvidenceIndex<'_>,
) -> Vec<CodeRepositorySetQueryHit> {
    let mut results = Vec::new();
    for outcome in outcomes {
        for hit in &outcome.hits {
            let overlay_evidence = edge_index.evidence_for_hit(hit);
            let score = repository_set_score(query, hit, &outcome.member_status, &overlay_evidence);
            results.push(CodeRepositorySetQueryHit {
                member: outcome.member_status.member.clone(),
                hit: hit.clone(),
                overlay_evidence,
                score,
            });
        }
    }

    results
}

fn repository_set_deferred_source_fallback_needed(
    request: &CodeRepositorySetQueryRequest,
    outcomes: &[RepositorySetMemberQueryOutcome],
    initial_results: &[CodeRepositorySetQueryHit],
) -> bool {
    if outcomes.iter().any(|outcome| {
        outcome.source_fallback_allowed
            && outcome.active_request.code_query_kind != CodeQueryKind::Hybrid
            && repository_set_member_source_fallback_needed(
                request,
                &outcome.active_request,
                outcome.hits.len(),
                outcome.dependency_symbol_plan_satisfied,
            )
    }) {
        return true;
    }
    if outcomes.iter().any(|outcome| {
        outcome.source_fallback_allowed
            && outcome.hits.is_empty()
            && repository_set_member_source_fallback_needed(
                request,
                &outcome.active_request,
                outcome.hits.len(),
                outcome.dependency_symbol_plan_satisfied,
            )
    }) {
        return true;
    }

    let mut ranked = initial_results.to_vec();
    dedupe_sort_truncate(&mut ranked, request.limit, &request.query);
    ranked.len() < request.limit.max(1)
}

async fn apply_repository_set_deferred_source_fallbacks(
    store: Arc<dyn crate::storage::KnowledgeStore>,
    request: &CodeRepositorySetQueryRequest,
    outcomes: &mut [RepositorySetMemberQueryOutcome],
) -> Result<(), ApiError> {
    let fallback_inputs = outcomes
        .iter()
        .enumerate()
        .filter(|(_, outcome)| {
            outcome.source_fallback_allowed
                && repository_set_member_source_fallback_needed(
                    request,
                    &outcome.active_request,
                    outcome.hits.len(),
                    outcome.dependency_symbol_plan_satisfied,
                )
        })
        .map(|(index, outcome)| RepositorySetMemberSourceFallbackInput {
            index,
            member_status: outcome.member_status.clone(),
            active_request: outcome.active_request.clone(),
            hits: outcome.hits.clone(),
        })
        .collect::<Vec<_>>();
    let fallback_outputs = stream::iter(fallback_inputs)
        .map(|input| {
            let store = Arc::clone(&store);
            async move { apply_repository_set_member_source_fallback(store, input).await }
        })
        .buffer_unordered(REPOSITORY_SET_QUERY_MEMBER_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for output in fallback_outputs {
        let output = output?;
        outcomes[output.index].hits = output.hits;
        outcomes[output.index].degraded_reason = output.degraded_reason;
    }

    Ok(())
}

async fn apply_repository_set_member_source_fallback(
    store: Arc<dyn crate::storage::KnowledgeStore>,
    input: RepositorySetMemberSourceFallbackInput,
) -> Result<RepositorySetMemberSourceFallbackOutput, ApiError> {
    let mut hits = input.hits;
    let base_status =
        required_member_repository(&store, &input.member_status.member.repository_id).await?;
    let scoped_member_status =
        code_status_for_repository_set_member(&base_status, &input.member_status);
    let degraded_reason = apply_code_grep_fallback(
        &store,
        &base_status,
        &scoped_member_status,
        &input.active_request,
        &mut hits,
    )
    .await?;

    Ok(RepositorySetMemberSourceFallbackOutput {
        index: input.index,
        hits,
        degraded_reason,
    })
}

fn repository_set_member_source_fallback_needed(
    set_request: &CodeRepositorySetQueryRequest,
    active_request: &CodeRetrievalRequest,
    hit_count: usize,
    dependency_symbol_plan_satisfied: bool,
) -> bool {
    if dependency_symbol_plan_satisfied {
        return false;
    }

    active_request.code_query_kind != CodeQueryKind::Hybrid || hit_count < set_request.limit.max(1)
}
fn join_degraded_reasons(reasons: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let mut joined = Vec::new();
    for reason in reasons.into_iter().flatten() {
        if !joined.contains(&reason) {
            joined.push(reason);
        }
    }

    (!joined.is_empty()).then(|| joined.join("; "))
}
fn unfresh_set_error_for_wait_policy(
    request: &CodeRepositorySetQueryRequest,
    status: &CodeRepositorySetStatus,
) -> Option<ApiError> {
    if request.freshness_policy != FreshnessPolicy::WaitUntilFresh {
        return None;
    }
    if status.members.is_empty() {
        return Some(ApiError::invalid_argument(format!(
            "code repository set '{}' has no members",
            status.repository_set.alias
        )));
    }
    if let Some(member) = status.members.iter().find(|member| member.stale) {
        return Some(ApiError::invalid_argument(format!(
            "code repository set '{}' member '{}' scope '{}' is stale",
            status.repository_set.alias, member.member.repository_alias, member.member.source_scope
        )));
    }
    if status.overlay.stale {
        return Some(ApiError::invalid_argument(format!(
            "code repository set '{}' overlay is stale; run repo-set refresh before querying with wait_until_fresh",
            status.repository_set.alias
        )));
    }

    None
}
async fn required_member_repository(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    repository_id: &str,
) -> Result<CodeRepositoryStatus, ApiError> {
    store
        .code_repository_status(repository_id.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository set member repository '{repository_id}' is not registered"
            ))
        })
}

fn code_status_for_repository_set_member(
    base_status: &CodeRepositoryStatus,
    member_status: &CodeRepositorySetMemberStatus,
) -> CodeRepositoryStatus {
    let member = &member_status.member;
    CodeRepositoryStatus {
        repository_id: member.repository_id.clone(),
        alias: member.repository_alias.clone(),
        root_path: base_status.root_path.clone(),
        path_filters: member.path_filters.clone(),
        language_filters: member.language_filters.clone(),
        last_indexed_scope_id: Some(member.source_scope.clone()),
        last_indexed_commit: Some(member.resolved_commit_sha.clone()),
        tree_hash: Some(member_status.tree_hash.clone()),
        state: member_status.freshness_state.clone(),
        indexed_file_count: member_status.indexed_file_count,
        symbol_count: member_status.symbol_count,
        reference_count: member_status.reference_count,
        chunk_count: member_status.chunk_count,
        stale: member_status.stale,
        degraded_reason: member_status.degraded_reason.clone(),
    }
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
