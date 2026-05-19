use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositorySetAddResponse, CodeRepositorySetCreateResponse,
        CodeRepositorySetQueryResponse, CodeRepositorySetRefreshResponse,
        CodeRepositorySetStatusResponse, RequestContext,
    },
    code::{CodeIndexError, resolve_repository_snapshot},
    domain::{
        CodeRepositorySelector, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetMemberStatus, CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest,
        CodeRepositorySetStatus, CodeRetrievalHit, CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetRefreshTaskClaimRequest,
        CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
        CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, StorageError,
    },
};
use std::collections::BTreeMap;

use super::RelayKnowledgeService;

const REPOSITORY_SET_REFRESH_TASK_LEASE_MS: u64 = 10 * 60 * 1000;
const REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS: u32 = 3;
const REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS: u64 = 60_000;

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
        let root_path = repository.root_path.clone();
        let ref_selector = request.ref_selector.clone();
        let (resolved_commit_sha, _tree_hash) =
            run_blocking_code(move || resolve_repository_snapshot(root_path, &ref_selector))
                .await?;
        let path_filters = merged_filters(&repository.path_filters, &request.path_filters);
        let language_filters =
            merged_filters(&repository.language_filters, &request.language_filters);
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
        let mut results = Vec::new();
        let candidate_limit = per_member_candidate_limit(request.limit);
        for member_status in &status.members {
            let member = &member_status.member;
            let selector = CodeRepositorySelector::new(
                member.repository_alias.clone(),
                member.resolved_commit_sha.clone(),
                merged_filters(&member.path_filters, &request.path_filters),
                merged_filters(&member.language_filters, &request.language_filters),
            )
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            let search_request = CodeRetrievalRequest::new(
                request.query.clone(),
                selector,
                request.code_query_kind,
                candidate_limit,
                FreshnessPolicy::AllowStale,
            )
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            let hits = store
                .search_code(search_request)
                .await
                .map_err(storage_api_error)?;
            for hit in hits {
                let overlay_evidence = overlay_evidence_for_hit(&edges, &hit);
                let score = repository_set_score(&hit, member_status, &overlay_evidence);
                results.push(CodeRepositorySetQueryHit {
                    member: member.clone(),
                    hit,
                    overlay_evidence,
                    score,
                });
            }
        }
        let truncated = dedupe_sort_truncate(&mut results, request.limit);
        let degraded_reason = status.degraded_reason.clone().or_else(|| {
            status
                .overlay
                .stale
                .then(|| "repository set overlay is stale".to_owned())
        });

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

    /// Checks whether a repository set selector exists.
    pub(crate) async fn code_repository_set_is_registered(
        &self,
        set_alias: String,
    ) -> Result<bool, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        store
            .code_repository_set(set_alias)
            .await
            .map(|set| set.is_some())
            .map_err(storage_api_error)
    }
}

async fn required_set_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    set_alias: &str,
) -> Result<CodeRepositorySetStatus, ApiError> {
    store
        .code_repository_set_status(set_alias.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository set '{set_alias}' is not registered"
            ))
        })
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

fn per_member_candidate_limit(limit: usize) -> usize {
    std::cmp::min(
        50,
        std::cmp::max(limit.saturating_mul(3), limit.saturating_add(5)),
    )
}

fn repository_set_score(
    hit: &CodeRetrievalHit,
    member: &CodeRepositorySetMemberStatus,
    overlay_evidence: &[crate::domain::CodeRepositoryCrossEdge],
) -> f64 {
    let priority_bonus = f64::from(member.member.priority) * 0.01;
    let freshness_penalty = if hit.stale || member.stale { 0.5 } else { 0.0 };
    let edge_bonus = overlay_evidence
        .iter()
        .map(|edge| f64::from(edge.confidence_basis_points) / 10_000.0)
        .fold(0.0, f64::max);

    hit.score + priority_bonus + edge_bonus - freshness_penalty
}

fn overlay_evidence_for_hit(
    edges: &[crate::domain::CodeRepositoryCrossEdge],
    hit: &CodeRetrievalHit,
) -> Vec<crate::domain::CodeRepositoryCrossEdge> {
    edges
        .iter()
        .filter(|edge| {
            edge.from_source_scope == hit.scope_id
                && evidence_path(edge.evidence_json.as_str()).as_deref() == Some(hit.path.as_str())
                || edge.to_source_scope.as_deref() == Some(hit.scope_id.as_str())
        })
        .take(5)
        .cloned()
        .collect()
}

fn evidence_path(evidence_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(evidence_json)
        .ok()
        .and_then(|value| {
            value
                .get("from_path")
                .and_then(|path| path.as_str())
                .map(str::to_owned)
        })
}

fn dedupe_sort_truncate(results: &mut Vec<CodeRepositorySetQueryHit>, limit: usize) -> bool {
    let mut best =
        BTreeMap::<(String, String, String, u32, u32, String), CodeRepositorySetQueryHit>::new();
    for result in results.drain(..) {
        let key = (
            result.hit.repository_id.clone(),
            result.hit.scope_id.clone(),
            result.hit.path.clone(),
            result.hit.line_range.start,
            result.hit.line_range.end,
            result.hit.excerpt.clone(),
        );
        match best.get(&key) {
            Some(existing) if existing.score >= result.score => {}
            _ => {
                best.insert(key, result);
            }
        }
    }
    results.extend(best.into_values());
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                left.member
                    .repository_alias
                    .cmp(&right.member.repository_alias)
            })
            .then_with(|| left.hit.path.cmp(&right.hit.path))
            .then_with(|| left.hit.line_range.start.cmp(&right.hit.line_range.start))
    });
    let truncated = results.len() > limit;
    results.truncate(limit);
    truncated
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

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

async fn run_blocking_code<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CodeIndexError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        .map_err(code_api_error)
}

fn code_api_error(error: CodeIndexError) -> ApiError {
    match error {
        CodeIndexError::InvalidInput(message) => ApiError::invalid_argument(message),
        CodeIndexError::Git { .. } | CodeIndexError::Io(_) | CodeIndexError::TreeSitter(_) => {
            ApiError::storage_unavailable(error.to_string())
        }
    }
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}
