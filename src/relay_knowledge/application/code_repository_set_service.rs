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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::ErrorKind,
        domain::{
            CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetMember,
            CodeRepositorySetOverlayStatus, CodeRetrievalLayer, RepositoryCodeRange,
        },
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
    fn helper_ranking_dedupes_and_attaches_overlay_evidence() {
        let member = member_status("app", "scope-app", 7);
        let base_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
        let inbound = edge(
            "edge-in",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            9_000,
        );
        let outbound = edge(
            "edge-out",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs"}"#,
            6_000,
        );
        let unrelated = edge(
            "edge-other",
            "scope-other",
            Some("scope-service"),
            r#"{"from_path":"src/other.rs"}"#,
            10_000,
        );
        let evidence =
            overlay_evidence_for_hit(&[inbound.clone(), outbound.clone(), unrelated], &base_hit);
        assert_eq!(evidence, vec![inbound, outbound]);
        assert!(repository_set_score(&base_hit, &member, &evidence) > base_hit.score);
        assert!(
            repository_set_score(
                &hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, true),
                &member,
                &[]
            ) < base_hit.score
        );

        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.50, false),
                overlay_evidence: Vec::new(),
                score: 0.50,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.90, false),
                overlay_evidence: evidence,
                score: 0.90,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 2, 0.80, false),
                overlay_evidence: Vec::new(),
                score: 0.80,
            },
        ];
        assert!(dedupe_sort_truncate(&mut results, 1));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.90);

        assert_eq!(per_member_candidate_limit(1), 6);
        assert_eq!(per_member_candidate_limit(20), 50);
        assert_eq!(
            merged_filters(&["src".to_owned()], &["src".to_owned(), "tests".to_owned()]),
            ["src".to_owned(), "tests".to_owned()]
        );
        assert!(evidence_path("not-json").is_none());
        assert!(evidence_path("{}").is_none());
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
            code_api_error(CodeIndexError::InvalidInput("bad ref".to_owned())).error_kind,
            ErrorKind::InvalidArgument
        );
        assert_eq!(
            code_api_error(CodeIndexError::Io(std::io::Error::other("disk"))).error_kind,
            ErrorKind::StorageUnavailable
        );
        assert_eq!(
            storage_api_error(StorageError::InvalidInput("bad storage".to_owned())).error_kind,
            ErrorKind::StorageUnavailable
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

    fn hit(
        repository_id: &str,
        scope_id: &str,
        path: &str,
        line: u32,
        score: f64,
        stale: bool,
    ) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: repository_id.to_owned(),
            scope_id: scope_id.to_owned(),
            resolved_commit_sha: format!("commit-{scope_id}"),
            tree_hash: format!("tree-{scope_id}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            symbol_snapshot_id: Some(format!("symbol-{line}")),
            canonical_symbol_id: None,
            file_id: Some("file-1".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Symbol],
            index_versions: vec!["code:1".to_owned()],
            stale,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score,
            excerpt: format!("excerpt {line}"),
        }
    }

    fn edge(
        edge_id: &str,
        from_scope: &str,
        to_scope: Option<&str>,
        evidence_json: &str,
        confidence: u16,
    ) -> CodeRepositoryCrossEdge {
        CodeRepositoryCrossEdge {
            edge_id: edge_id.to_owned(),
            set_id: "set-workspace".to_owned(),
            from_source_scope: from_scope.to_owned(),
            from_repository_id: "repo-from".to_owned(),
            from_record_kind: "module_reference".to_owned(),
            from_record_id: "import-1".to_owned(),
            to_source_scope: to_scope.map(str::to_owned),
            to_repository_id: to_scope.map(|_| "repo-to".to_owned()),
            to_record_kind: "code_symbol_snapshot".to_owned(),
            to_record_id: to_scope.map(|_| "symbol-1".to_owned()),
            edge_kind: "imports".to_owned(),
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: confidence,
            confidence_tier: "explicit".to_owned(),
            evidence_json: evidence_json.to_owned(),
            created_at_ms: 10,
        }
    }
}
