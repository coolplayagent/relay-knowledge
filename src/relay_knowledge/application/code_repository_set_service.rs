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
        CodeRepositorySetStatus, CodeRepositoryStatus, CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetRefreshTaskClaimRequest,
        CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
        CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, StorageError,
    },
};
use std::path::PathBuf;

use super::{
    RelayKnowledgeService,
    code_repository_set_plan::{
        dependency_symbol_plan_needs_hybrid_fallback, repository_set_member_query_plan,
    },
    code_repository_set_query::{
        OverlayEvidenceIndex, apply_bridge_support_bonus, dedupe_sort_truncate,
        per_member_candidate_limit, prune_returned_overlay_evidence, repository_set_score,
    },
    code_service::apply_code_grep_fallback,
};

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
        let edge_index = OverlayEvidenceIndex::new(&edges);
        let mut results = Vec::new();
        let mut fallback_degraded_reason = None;
        let candidate_limit = per_member_candidate_limit(request.limit, status.members.len());
        let highest_priority = status
            .members
            .iter()
            .map(|member| member.member.priority)
            .max()
            .unwrap_or(0);
        for member_status in &status.members {
            let member = &member_status.member;
            let selector = CodeRepositorySelector::new(
                member.repository_alias.clone(),
                member.resolved_commit_sha.clone(),
                request.path_filters.clone(),
                request.language_filters.clone(),
            )
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            let member_query_plan =
                repository_set_member_query_plan(&request, member_status, highest_priority);
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
            if dependency_symbol_plan_needs_hybrid_fallback(&request, member_query_plan.kind, &hits)
            {
                let fallback_request = CodeRetrievalRequest::new(
                    request.query.clone(),
                    selector,
                    request.code_query_kind,
                    candidate_limit,
                    FreshnessPolicy::AllowStale,
                )
                .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
                active_request = fallback_request.clone();
                hits = store
                    .search_code_scope(member.source_scope.clone(), fallback_request)
                    .await
                    .map_err(storage_api_error)?;
            }
            let base_status = required_member_repository(&store, &member.repository_id).await?;
            let scoped_member_status =
                code_status_for_repository_set_member(&base_status, member_status);
            fallback_degraded_reason = fallback_degraded_reason.or(apply_code_grep_fallback(
                &store,
                &base_status,
                &scoped_member_status,
                &active_request,
                &mut hits,
            )
            .await?);
            for hit in hits {
                let overlay_evidence = edge_index.evidence_for_hit(&hit);
                let score = repository_set_score(&hit, member_status, &overlay_evidence);
                results.push(CodeRepositorySetQueryHit {
                    member: member.clone(),
                    hit,
                    overlay_evidence,
                    score,
                });
            }
        }
        apply_bridge_support_bonus(&mut results);
        let truncated = dedupe_sort_truncate(&mut results, request.limit, &request.query);
        prune_returned_overlay_evidence(&mut results);
        let degraded_reason = join_degraded_reasons([
            status.degraded_reason.clone(),
            status
                .overlay
                .stale
                .then(|| "repository set overlay is stale".to_owned()),
            fallback_degraded_reason,
        ]);

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
    let root_path = PathBuf::from(repository.root_path);
    let ref_selector = member.ref_selector.clone();
    let resolved =
        tokio::task::spawn_blocking(move || resolve_repository_snapshot(root_path, &ref_selector))
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;

    match resolved {
        Ok((current_commit, _)) if current_commit == member.resolved_commit_sha => Ok(None),
        Ok((current_commit, _)) => Ok(Some(format!(
            "repository set member '{}' ref '{}' now resolves to {}, not stored snapshot {}",
            member.repository_alias,
            member.ref_selector,
            current_commit,
            member.resolved_commit_sha
        ))),
        Err(error) => Ok(Some(format!(
            "repository set member '{}' ref '{}' could not be resolved: {error}",
            member.repository_alias, member.ref_selector
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

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    match error {
        StorageError::InvalidInput(message) => ApiError::invalid_argument(message),
        other => ApiError::storage_unavailable(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::ErrorKind,
        domain::CodeRepositorySetMemberStatus,
        domain::{CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetOverlayStatus},
        storage::SqliteGraphStore,
    };
    use std::sync::Arc;

    #[test]
    fn helper_policy_reports_wait_until_fresh_blockers() {
        let request = CodeRepositorySetQueryRequest::new(
            "workspace",
            "serve",
            crate::domain::CodeQueryKind::Definition,
            5,
            FreshnessPolicy::WaitUntilFresh,
            Vec::new(),
            Vec::new(),
        )
        .expect("request should validate");
        let empty = status_with_members(Vec::new(), overlay(true));
        assert!(
            unfresh_set_error_for_wait_policy(&request, &empty)
                .expect("empty set should block")
                .message
                .contains("has no members")
        );

        let mut stale_member = member_status("app", "scope-app", 0);
        stale_member.stale = true;
        let stale_status = status_with_members(vec![stale_member], overlay(false));
        assert!(
            unfresh_set_error_for_wait_policy(&request, &stale_status)
                .expect("stale member should block")
                .message
                .contains("member 'app'")
        );

        let overlay_status =
            status_with_members(vec![member_status("app", "scope-app", 0)], overlay(true));
        assert!(
            unfresh_set_error_for_wait_policy(&request, &overlay_status)
                .expect("stale overlay should block")
                .message
                .contains("overlay is stale")
        );

        let allow_stale = CodeRepositorySetQueryRequest::new(
            "workspace",
            "serve",
            crate::domain::CodeQueryKind::Definition,
            5,
            FreshnessPolicy::AllowStale,
            Vec::new(),
            Vec::new(),
        )
        .expect("request should validate");
        assert!(unfresh_set_error_for_wait_policy(&allow_stale, &overlay_status).is_none());
    }

    #[test]
    fn helper_fingerprint_and_error_mapping_are_stable() {
        let status = status_with_members(
            vec![
                member_status("app", "scope-app", 1),
                member_status("svc", "scope-svc", 0),
            ],
            overlay(false),
        );
        let fingerprint = repository_set_refresh_fingerprint(&status);
        assert!(fingerprint.contains("set-workspace"));
        assert!(fingerprint.contains("repo-app:scope-app:commit-scope-app:tree-scope-app:false"));
        assert_eq!(
            merged_filters(&["src".to_owned()], &["src".to_owned(), "tests".to_owned()]),
            ["src".to_owned(), "tests".to_owned()]
        );
        assert_eq!(
            code_api_error(CodeIndexError::InvalidInput("bad ref".to_owned())).error_kind,
            ErrorKind::InvalidArgument
        );
        assert_eq!(
            code_api_error(CodeIndexError::Io(std::io::Error::other("disk"))).error_kind,
            ErrorKind::StorageUnavailable
        );
        assert_eq!(
            storage_api_error(StorageError::InvalidInput("bad storage".to_owned())).error_kind,
            ErrorKind::InvalidArgument
        );
    }

    #[tokio::test]
    async fn helper_required_status_reports_missing_sets() {
        let store: Arc<dyn crate::storage::KnowledgeStore> =
            Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
        let error = required_set_status(&store, "missing")
            .await
            .expect_err("missing set should fail");

        assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
        assert!(error.message.contains("is not registered"));
    }

    fn status_with_members(
        members: Vec<CodeRepositorySetMemberStatus>,
        overlay: CodeRepositorySetOverlayStatus,
    ) -> CodeRepositorySetStatus {
        CodeRepositorySetStatus {
            repository_set: CodeRepositorySet {
                set_id: "set-workspace".to_owned(),
                alias: "workspace".to_owned(),
                description: None,
                default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            members,
            overlay,
            freshness_state: "fresh".to_owned(),
            degraded_reason: None,
        }
    }

    fn member_status(
        repository_alias: &str,
        source_scope: &str,
        priority: i32,
    ) -> CodeRepositorySetMemberStatus {
        CodeRepositorySetMemberStatus {
            member: CodeRepositorySetMember {
                set_id: "set-workspace".to_owned(),
                repository_id: format!("repo-{repository_alias}"),
                repository_alias: repository_alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: format!("commit-{source_scope}"),
                source_scope: source_scope.to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
                priority,
            },
            tree_hash: format!("tree-{source_scope}"),
            freshness_state: "fresh".to_owned(),
            stale: false,
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            degraded_reason: None,
        }
    }

    fn overlay(stale: bool) -> CodeRepositorySetOverlayStatus {
        CodeRepositorySetOverlayStatus {
            state: if stale { "overlay_stale" } else { "fresh" }.to_owned(),
            stale,
            edge_count: usize::from(!stale),
            refreshed_at_ms: (!stale).then_some(10),
            degraded_reason: None,
        }
    }
}
