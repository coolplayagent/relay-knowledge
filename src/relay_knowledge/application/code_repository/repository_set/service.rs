use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositorySetAddResponse, CodeRepositorySetCreateResponse,
        CodeRepositorySetQueryResponse, CodeRepositorySetRefreshResponse,
        CodeRepositorySetStatusResponse, RequestContext,
    },
    domain::{
        CodeQueryKind, CodeRepositorySelector, CodeRepositorySetAddMemberRequest,
        CodeRepositorySetCreateRequest, CodeRepositorySetMemberStatus, CodeRepositorySetQueryHit,
        CodeRepositorySetQueryRequest, CodeRepositorySetStatus, CodeRepositoryStatus,
        CodeRetrievalHit, CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetRefreshTaskClaimRequest,
        CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
        CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, StorageError,
    },
};
use futures_util::{StreamExt, stream};
use std::sync::Arc;

use crate::application::{
    code_repository::support::{apply_code_grep_fallback, resolve_code_ref_for_selector},
    service::RelayKnowledgeService,
};

#[cfg(test)]
use crate::code::CodeIndexError;

use super::{
    plan::{
        dependency_symbol_plan_needs_hybrid_fallback, merge_dependency_symbol_fallback_hits,
        repository_set_member_query_plan,
    },
    query::{
        OverlayEvidenceIndex, apply_bridge_support_bonus, dedupe_sort_truncate,
        per_member_candidate_limit, prune_returned_overlay_evidence, repository_set_score,
    },
};

const REPOSITORY_SET_REFRESH_TASK_LEASE_MS: u64 = 10 * 60 * 1000;
const REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS: u32 = 3;
const REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
const REPOSITORY_SET_QUERY_MEMBER_CONCURRENCY: usize = 4;

impl RelayKnowledgeService {
    /// Creates or updates a thin repository set.
    pub async fn create_code_repository_set(
        &self,
        request: CodeRepositorySetCreateRequest,
        context: RequestContext,
    ) -> Result<CodeRepositorySetCreateResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let repository_set = store
            .create_code_repository_set(CodeRepositorySetSeed {
                alias: request.alias.clone(),
                description: request.description.clone(),
                default_ref_policy_json: request.default_ref_policy_json.clone(),
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetCreateResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            request,
            repository_set,
        })
    }

    /// Adds one already-indexed repository snapshot to a repository set.
    pub async fn add_code_repository_set_member(
        &self,
        request: CodeRepositorySetAddMemberRequest,
        context: RequestContext,
    ) -> Result<CodeRepositorySetAddResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let repository = store
            .code_repository_status(request.repository_alias.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' is not registered",
                    request.repository_alias
                ))
            })?;
        let path_filters = merged_filters(&repository.path_filters, &request.path_filters);
        let language_filters =
            merged_filters(&repository.language_filters, &request.language_filters);
        let selector = CodeRepositorySelector {
            repository: request.repository_alias.clone(),
            ref_selector: request.ref_selector.clone(),
            path_filters: request.path_filters.clone(),
            language_filters: request.language_filters.clone(),
        };
        let resolved_commit_sha =
            resolve_code_ref_for_selector(&repository, &selector, request.ref_selector.clone())
                .await?;
        let scope = store
            .code_repository_scope_status(
                request.repository_alias.clone(),
                resolved_commit_sha.clone(),
                path_filters.clone(),
                language_filters.clone(),
            )
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' has no indexed scope for ref {} and requested filters",
                    request.repository_alias, request.ref_selector
                ))
            })?;
        let source_scope = scope.last_indexed_scope_id.clone().ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository '{}' matching scope has no source scope",
                request.repository_alias
            ))
        })?;
        let member = store
            .add_code_repository_set_member(CodeRepositorySetMemberSeed {
                set_alias: request.set_alias.clone(),
                repository_id: repository.repository_id,
                repository_alias: request.repository_alias.clone(),
                ref_selector: request.ref_selector.clone(),
                resolved_commit_sha,
                source_scope,
                path_filters,
                language_filters,
                priority: request.priority,
            })
            .await
            .map_err(storage_api_error)?;
        let status = required_set_status(&store, &request.set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetAddResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            request,
            member,
            status,
        })
    }

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
        results.extend(repository_set_results_from_outcomes(&outcomes, &edge_index));
        apply_bridge_support_bonus(&mut results);
        if repository_set_deferred_source_fallback_needed(&request, &outcomes, &results) {
            apply_repository_set_deferred_source_fallbacks(
                Arc::clone(&store),
                &request,
                &mut outcomes,
            )
            .await?;
            results.clear();
            results.extend(repository_set_results_from_outcomes(&outcomes, &edge_index));
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

    /// Returns repository-set freshness and member diagnostics.
    pub async fn code_repository_set_status(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetStatusResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
        })
    }

    /// Rebuilds cross-repository import/module overlay edges.
    pub async fn refresh_code_repository_set(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let summary = store
            .refresh_code_repository_set_overlay(set_alias.clone(), now_millis())
            .await
            .map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetRefreshResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            summary: Some(summary),
            task: None,
        })
    }

    /// Queues a repository-set overlay refresh task.
    pub async fn start_code_repository_set_refresh(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let fingerprint = repository_set_refresh_fingerprint(&status);
        let task = store
            .queue_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskSeed {
                set_id: status.repository_set.set_id.clone(),
                set_alias: status.repository_set.alias.clone(),
                input_fingerprint: fingerprint,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetRefreshResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            summary: None,
            task: Some(task),
        })
    }

    /// Runs one queued repository-set overlay refresh task under a lease.
    pub async fn run_code_repository_set_refresh_task_once(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<crate::domain::CodeRepositorySetRefreshTaskRecord>, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let lease_owner = format!("code-repository-set-refresh-worker-{}", std::process::id());
        let Some(task) = store
            .claim_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskClaimRequest {
                task_id,
                lease_owner: lease_owner.clone(),
                lease_duration_ms: REPOSITORY_SET_REFRESH_TASK_LEASE_MS,
                max_attempts: REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?
        else {
            return Ok(None);
        };
        let result = self
            .refresh_code_repository_set(task.set_alias.clone(), context)
            .await;
        match result {
            Ok(_) => store
                .complete_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskCompletion {
                    task_id: task.task_id,
                    lease_owner,
                    attempt_count: task.attempt_count,
                    now_ms: now_millis(),
                })
                .await
                .map(Some)
                .map_err(storage_api_error),
            Err(error) => {
                let _ = store
                    .fail_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
                        error_kind: "repository_set_overlay".to_owned(),
                        error_message: error.message.clone(),
                        retry_backoff_ms: REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS,
                        max_attempts: REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS,
                        now_ms: now_millis(),
                    })
                    .await;
                Err(error)
            }
        }
    }

    pub(crate) async fn code_repository_set_member_scopes(
        &self,
        set_alias: String,
    ) -> Result<Option<Vec<(String, String)>>, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        store
            .code_repository_set_status(set_alias)
            .await
            .map(|status| {
                status.map(|status| {
                    status
                        .members
                        .into_iter()
                        .map(|member| (member.member.repository_alias, member.member.source_scope))
                        .collect()
                })
            })
            .map_err(storage_api_error)
    }
}

struct RepositorySetMemberQueryOutcome {
    member_status: CodeRepositorySetMemberStatus,
    hits: Vec<CodeRetrievalHit>,
    active_request: CodeRetrievalRequest,
    dependency_symbol_plan_satisfied: bool,
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
    let search_request = CodeRetrievalRequest::new(
        member_query_plan.query,
        selector.clone(),
        member_query_plan.kind,
        candidate_limit,
        FreshnessPolicy::AllowStale,
    )
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut active_request = search_request.clone();
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
        let fallback_request = CodeRetrievalRequest::new(
            request.query.clone(),
            selector,
            request.code_query_kind,
            candidate_limit,
            FreshnessPolicy::AllowStale,
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
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
        degraded_reason: None,
    })
}

fn repository_set_results_from_outcomes(
    outcomes: &[RepositorySetMemberQueryOutcome],
    edge_index: &OverlayEvidenceIndex<'_>,
) -> Vec<CodeRepositorySetQueryHit> {
    let mut results = Vec::new();
    for outcome in outcomes {
        for hit in &outcome.hits {
            let overlay_evidence = edge_index.evidence_for_hit(hit);
            let score = repository_set_score(hit, &outcome.member_status, &overlay_evidence);
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
        outcome.active_request.code_query_kind != CodeQueryKind::Hybrid
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
        outcome.hits.is_empty()
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
            repository_set_member_source_fallback_needed(
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

pub(super) async fn required_set_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    set_alias: &str,
) -> Result<CodeRepositorySetStatus, ApiError> {
    let mut status = store
        .code_repository_set_status(set_alias.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository set '{set_alias}' is not registered"
            ))
        })?;
    refresh_moving_member_freshness(store, &mut status).await?;
    refresh_repository_set_freshness(&mut status);

    Ok(status)
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

async fn refresh_moving_member_freshness(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &mut CodeRepositorySetStatus,
) -> Result<(), ApiError> {
    for index in 0..status.members.len() {
        let member = status.members[index].member.clone();
        let Some(reason) = moving_member_stale_reason(store, &member).await? else {
            continue;
        };
        status.members[index].stale = true;
        status.members[index].freshness_state = "stale".to_owned();
        status.members[index].degraded_reason = Some(reason);
    }

    Ok(())
}

async fn moving_member_stale_reason(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    member: &crate::domain::CodeRepositorySetMember,
) -> Result<Option<String>, ApiError> {
    if !member_ref_tracks_repository(&member.ref_selector, &member.resolved_commit_sha) {
        return Ok(None);
    }
    let repository = store
        .code_repository_status(member.repository_id.clone())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository '{}' is not registered",
                member.repository_alias
            ))
        })?;
    let ref_selector = member.ref_selector.clone();
    let selector = CodeRepositorySelector {
        repository: member.repository_alias.clone(),
        ref_selector: ref_selector.clone(),
        path_filters: member.path_filters.clone(),
        language_filters: member.language_filters.clone(),
    };
    let resolved = resolve_code_ref_for_selector(&repository, &selector, ref_selector).await;

    match resolved {
        Ok(current_commit) if current_commit == member.resolved_commit_sha => Ok(None),
        Ok(current_commit) => Ok(Some(format!(
            "repository set member '{}' ref '{}' now resolves to {}, not stored snapshot {}",
            member.repository_alias,
            member.ref_selector,
            current_commit,
            member.resolved_commit_sha
        ))),
        Err(error) => Ok(Some(format!(
            "repository set member '{}' ref '{}' could not be resolved: {error}",
            member.repository_alias,
            member.ref_selector,
            error = error.message
        ))),
    }
}

fn member_ref_tracks_repository(ref_selector: &str, resolved_commit_sha: &str) -> bool {
    let ref_selector = ref_selector.trim();
    !(ref_selector == resolved_commit_sha
        || (is_git_oid_prefix(ref_selector) && resolved_commit_sha.starts_with(ref_selector)))
}

fn is_git_oid_prefix(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn refresh_repository_set_freshness(status: &mut CodeRepositorySetStatus) {
    let member_stale = status.members.iter().any(|member| member.stale);
    if member_stale && !status.overlay.stale {
        status.overlay.stale = true;
        status.overlay.state = "overlay_stale".to_owned();
    }
    status.freshness_state = if status.members.is_empty() {
        "incomplete"
    } else if member_stale {
        "stale"
    } else if status.overlay.stale {
        "overlay_stale"
    } else {
        "fresh"
    }
    .to_owned();
    status.degraded_reason = status
        .members
        .iter()
        .find_map(|member| member.degraded_reason.clone())
        .or_else(|| status.overlay.degraded_reason.clone());
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

fn repository_set_refresh_fingerprint(status: &CodeRepositorySetStatus) -> String {
    let mut parts = vec![status.repository_set.set_id.clone()];
    parts.extend(status.members.iter().map(|member| {
        format!(
            "{}:{}:{}:{}:{}",
            member.member.repository_id,
            member.member.source_scope,
            member.member.resolved_commit_sha,
            member.tree_hash,
            member.stale
        )
    }));
    parts.join("|")
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
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

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
fn code_api_error(error: CodeIndexError) -> ApiError {
    match error {
        CodeIndexError::InvalidInput(message) => ApiError::invalid_argument(message),
        CodeIndexError::Git { .. } | CodeIndexError::Io(_) | CodeIndexError::TreeSitter(_) => {
            ApiError::storage_unavailable(error.to_string())
        }
    }
}

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    match error {
        StorageError::InvalidInput(message) => ApiError::invalid_argument(message),
        other => ApiError::storage_unavailable(other.to_string()),
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
