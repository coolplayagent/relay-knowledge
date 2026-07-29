use std::collections::BTreeSet;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryFreshnessDiagnostics, CodeRepositoryFreshnessInput,
        CodebaseViewResponse, RequestContext,
    },
    application::service::RelayKnowledgeService,
    domain::{
        CodeRepositorySelector, CodeRepositoryStatus, CodebaseViewKind, CodebaseViewRequest,
        CodebaseViewSnapshot, FreshnessPolicy,
    },
};

use super::super::{
    errors::storage_api_error,
    repository::{
        code_status_checkpoint, ensure_worktree_overlay_matches_current_worktree,
        required_code_repository,
    },
    scope::{
        active_index_matches_request, indexed_commit_for_selector, indexed_source_scope,
        latest_compatible_code_scope_status, missing_indexed_source_scope_error,
        resolved_code_scope_status,
    },
};
use super::{
    affected_scope::derive_affected_scope,
    architecture::derive_architecture_layers,
    builder::{DerivedView, ViewBuilder},
    business_domains::derive_business_domains,
    dependency_tour::derive_dependency_tour,
    process_flow::derive_process_flow,
    rules::normalized_view_paths,
};

const SNAPSHOT_LIMIT_MULTIPLIER: usize = 20;
const SNAPSHOT_LIMIT_MAX: usize = 2_000;

impl RelayKnowledgeService {
    /// Builds a deterministic, evidence-backed repository understanding view.
    pub async fn codebase_view(
        &self,
        request: CodebaseViewRequest,
        context: RequestContext,
    ) -> Result<CodebaseViewResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        let requested_ref = request.repository.ref_selector.clone();
        let mut request = view_request_at_indexed_ref(request, &status).await?;
        if requested_ref == "worktree" {
            ensure_worktree_overlay_matches_current_worktree(&store, &status, &request.repository)
                .await?;
        }
        let requested_resolved_ref = request.repository.ref_selector.clone();
        let freshness_target = request.repository.clone();
        let mut stale_reason = None;
        let scoped_status = match resolved_code_scope_status(&store, &status, &request.repository)
            .await
        {
            Ok(scoped_status) => scoped_status,
            Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                if !active_index_matches_request(&store, &status, &request.repository).await? {
                    return Err(error);
                }
                let Some(stale_status) =
                    latest_compatible_code_scope_status(&store, &request.repository).await?
                else {
                    return Err(error);
                };
                let Some(last_indexed_commit) = stale_status.last_indexed_commit.clone() else {
                    return Err(error);
                };
                request.repository.ref_selector = last_indexed_commit;
                stale_reason = Some(
                    "requested ref is not indexed yet; served last completed code index".to_owned(),
                );
                stale_status
            }
            Err(error) => return Err(error),
        };
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' scope '{}' is stale; run repo index before deriving codebase views with wait_until_fresh",
                scoped_status.alias,
                scoped_status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let row_limit = request
            .limit
            .saturating_mul(SNAPSHOT_LIMIT_MULTIPLIER)
            .min(SNAPSHOT_LIMIT_MAX);
        let snapshot = store
            .codebase_view_snapshot(source_scope, request.clone(), row_limit)
            .await
            .map_err(storage_api_error)?;
        let derived = derive_view(&request, snapshot, row_limit);
        let direct_source_read_paths = view_source_read_paths(&request, &derived);
        let degraded_reason = scoped_status
            .degraded_reason
            .clone()
            .or_else(|| stale_reason.clone());
        let scope_stale = served_scope_is_stale(&scoped_status, &stale_reason);
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if scope_stale {
            metadata.stale = true;
        }
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref.clone(),
        );
        if scope_stale {
            scope.stale = true;
        }
        let freshness = view_freshness(ViewFreshnessInput {
            store: &store,
            base_status: &status,
            scoped_status: &scoped_status,
            request: &request,
            requested_ref,
            requested_resolved_ref,
            freshness_target,
            stale_reason,
            degraded_reason: degraded_reason.clone(),
            graph_version: graph_version.get(),
            direct_source_read_paths,
        })
        .await?;

        Ok(CodebaseViewResponse {
            metadata,
            scope,
            freshness,
            request,
            graph_version: graph_version.get(),
            nodes: derived.nodes,
            edges: derived.edges,
            sections: derived.sections,
            evidence: derived.evidence,
            budget: derived.budget,
            diagnostics: derived.diagnostics,
            degraded_reason,
        })
    }
}

pub(super) fn derive_view(
    request: &CodebaseViewRequest,
    snapshot: CodebaseViewSnapshot,
    row_limit: usize,
) -> DerivedView {
    let mut builder = ViewBuilder::new(request.limit, row_limit, snapshot.truncated);
    match request.view_kind {
        CodebaseViewKind::ArchitectureLayers => derive_architecture_layers(&mut builder, &snapshot),
        CodebaseViewKind::BusinessDomains => derive_business_domains(&mut builder, &snapshot),
        CodebaseViewKind::DependencyTour => derive_dependency_tour(&mut builder, &snapshot),
        CodebaseViewKind::ProcessFlow => derive_process_flow(&mut builder, &snapshot),
        CodebaseViewKind::AffectedScope => derive_affected_scope(&mut builder, request, &snapshot),
    }
    builder.finish()
}

async fn view_request_at_indexed_ref(
    mut request: CodebaseViewRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodebaseViewRequest, ApiError> {
    request.repository.ref_selector = indexed_commit_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;

    Ok(request)
}

struct ViewFreshnessInput<'a> {
    store: &'a std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    base_status: &'a CodeRepositoryStatus,
    scoped_status: &'a CodeRepositoryStatus,
    request: &'a CodebaseViewRequest,
    requested_ref: String,
    requested_resolved_ref: String,
    freshness_target: CodeRepositorySelector,
    stale_reason: Option<String>,
    degraded_reason: Option<String>,
    graph_version: u64,
    direct_source_read_paths: Vec<String>,
}

async fn view_freshness(
    input: ViewFreshnessInput<'_>,
) -> Result<CodeRepositoryFreshnessDiagnostics, ApiError> {
    let active_task = input
        .store
        .active_code_index_task(input.base_status.repository_id.clone())
        .await
        .map_err(storage_api_error)?;
    let queue = input
        .store
        .code_index_task_queue_status()
        .await
        .map_err(storage_api_error)?;
    let active_matches_request =
        active_index_matches_request(input.store, input.base_status, &input.freshness_target)
            .await?;
    let pending = crate::api::CodeRepositoryPendingIndexWork::from_task_and_queue(
        active_task.as_ref(),
        active_matches_request,
        queue,
    );
    let checkpoint = if active_matches_request {
        code_status_checkpoint(input.store, input.scoped_status, active_task.as_ref()).await?
    } else if let Some(scope) = input.scoped_status.last_indexed_scope_id.clone() {
        input
            .store
            .code_index_checkpoint(scope)
            .await
            .map_err(storage_api_error)?
    } else {
        None
    };
    let cursor = checkpoint
        .as_ref()
        .map(crate::api::CodeRepositoryFreshnessCursor::from_checkpoint);
    let served_ref = input
        .scoped_status
        .last_indexed_commit
        .clone()
        .unwrap_or_else(|| input.request.repository.ref_selector.clone());

    Ok(CodeRepositoryFreshnessDiagnostics::code_query(
        CodeRepositoryFreshnessInput {
            graph_version: input.graph_version,
            freshness_policy: input.request.freshness_policy,
            source_scope: indexed_source_scope(input.scoped_status),
            requested_ref: input.requested_ref,
            requested_resolved_ref: input.requested_resolved_ref,
            served_ref,
            scope_stale: served_scope_is_stale(input.scoped_status, &input.stale_reason),
            stale_reason: input.stale_reason,
            degraded_reason: input.degraded_reason,
            pending,
            cursor,
            direct_source_read_paths: input.direct_source_read_paths,
        },
    ))
}

pub(super) fn view_source_read_paths(
    request: &CodebaseViewRequest,
    derived: &DerivedView,
) -> Vec<String> {
    let mut paths = BTreeSet::new();
    if request.view_kind == CodebaseViewKind::AffectedScope {
        paths.extend(normalized_view_paths(&request.changed_paths));
    }
    paths.extend(
        derived
            .evidence
            .iter()
            .map(|evidence| evidence.path.clone())
            .filter(|path| !path.is_empty()),
    );
    paths.extend(
        derived
            .nodes
            .iter()
            .filter_map(|node| node.path.clone())
            .filter(|path| !path.is_empty()),
    );
    paths.into_iter().collect()
}

pub(super) fn served_scope_is_stale(
    status: &CodeRepositoryStatus,
    stale_reason: &Option<String>,
) -> bool {
    status.stale || stale_reason.is_some()
}
