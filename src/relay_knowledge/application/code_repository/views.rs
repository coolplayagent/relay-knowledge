use std::collections::{BTreeMap, BTreeSet};

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryFreshnessDiagnostics, CodeRepositoryFreshnessInput,
        CodebaseViewResponse, RequestContext,
    },
    application::service::RelayKnowledgeService,
    domain::{
        CodeImportRecord, CodeRepositorySelector, CodeRepositoryStatus, CodeRouteRecord,
        CodebaseViewBudget, CodebaseViewCall, CodebaseViewKind, CodebaseViewRequest,
        CodebaseViewSnapshot, FreshnessPolicy,
    },
};

use super::support::{
    active_index_matches_request, code_status_checkpoint, indexed_commit_for_selector,
    indexed_source_scope, latest_compatible_code_scope_status, missing_indexed_source_scope_error,
    required_code_repository, resolved_code_scope_status, storage_api_error,
};
use super::worktree_freshness::ensure_worktree_overlay_matches_current_worktree;
use views_builder::{DerivedView, SectionRefs, ViewBuilder};
use views_dependency_tour::derive_dependency_tour;
use views_rules::{
    affected_candidate_matches_changed_path, architecture_layer, domain_confidence, domain_token,
    is_test_config_or_doc, layer_confidence, module_key, normalized_view_paths, path_domain,
    route_domain,
};

#[path = "views_builder.rs"]
mod views_builder;
#[path = "views_dependency_tour.rs"]
mod views_dependency_tour;
#[path = "views_rules.rs"]
mod views_rules;

const SNAPSHOT_LIMIT_MULTIPLIER: usize = 20;
const SNAPSHOT_LIMIT_MAX: usize = 2_000;
const PROCESS_FLOW_CALL_LIMIT: usize = 8;

impl RelayKnowledgeService {
    /// Builds a deterministic, evidence-backed repository understanding view.
    pub async fn codebase_view(
        &self,
        request: CodebaseViewRequest,
        context: RequestContext,
    ) -> Result<CodebaseViewResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            return Ok(empty_view_response(
                &context,
                graph_version.get(),
                &status,
                request,
                degraded_reason,
            ));
        }

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

fn derive_view(
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

fn derive_architecture_layers(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    let indexed_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut layer_candidates = BTreeMap::<String, (Vec<&str>, Vec<String>)>::new();
    for file in &snapshot.files {
        let layer = architecture_layer(&file.path);
        let evidence_id = builder.evidence(
            "file",
            &file.path,
            None,
            None,
            None,
            format!("{} file in {layer} layer", file.language_id),
        );
        let (files, evidence_ids) = layer_candidates.entry(layer.to_owned()).or_default();
        files.push(file.path.as_str());
        evidence_ids.push(evidence_id);
    }
    let mut ordered_layers = layer_candidates.into_iter().collect::<Vec<_>>();
    ordered_layers
        .sort_by_cached_key(|(layer, (files, _))| (std::cmp::Reverse(files.len()), layer.clone()));
    let mut layer_files = BTreeMap::<String, Vec<&str>>::new();
    let mut layer_evidence = BTreeMap::<String, Vec<String>>::new();
    if ordered_layers.len() > builder.limit {
        builder.mark_node_budget_truncated();
    }
    for (layer, (files, evidence_ids)) in ordered_layers.into_iter().take(builder.limit) {
        let Some(node_id) = builder.node(
            format!("layer:{layer}"),
            layer.clone(),
            "architecture_layer",
            None,
            layer_confidence(&layer),
            evidence_ids.first().cloned(),
        ) else {
            continue;
        };
        layer_files.insert(node_id.clone(), files);
        layer_evidence.insert(node_id, evidence_ids);
    }
    for import in &snapshot.imports {
        if let Some(target_path) = resolved_indexed_import_target(import, &indexed_paths) {
            let source = format!("layer:{}", architecture_layer(&import.path));
            let target = format!("layer:{}", architecture_layer(target_path));
            let evidence_id = builder.evidence(
                "import",
                &import.path,
                Some(import.module.clone()),
                Some(import.line_range.clone()),
                Some(import.resolution_state.clone()),
                "import edge between architecture layers",
            );
            let source_id = builder.node(
                source.clone(),
                source.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            let target_id = builder.node(
                target.clone(),
                target.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            if let (Some(source_id), Some(target_id)) = (source_id, target_id) {
                builder.edge(&source_id, &target_id, "imports", 0.72, Some(evidence_id));
            }
        }
    }
    for call in &snapshot.calls {
        if let Some(target_path) = call.callee_path.as_deref() {
            let source = format!("layer:{}", architecture_layer(&call.call.path));
            let target = format!("layer:{}", architecture_layer(target_path));
            let evidence_id = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                format!("call to {}", call.call.callee_name),
            );
            let source_id = builder.node(
                source.clone(),
                source.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            let target_id = builder.node(
                target.clone(),
                target.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            if let (Some(source_id), Some(target_id)) = (source_id, target_id) {
                builder.edge(&source_id, &target_id, "calls", 0.76, Some(evidence_id));
            }
        }
    }
    let mut ordered_layers = layer_files.into_iter().collect::<Vec<_>>();
    ordered_layers
        .sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    for (node_id, files) in ordered_layers.into_iter().take(builder.limit) {
        let layer = node_id.trim_start_matches("layer:");
        let evidence_ids = layer_evidence.remove(&node_id).unwrap_or_default();
        builder.section(
            format!("section:{node_id}"),
            format!("{layer} layer"),
            format!(
                "{layer} contains {} indexed file(s) and is derived from path and graph boundary evidence.",
                files.len()
            ),
            layer_confidence(layer),
            SectionRefs {
                node_ids: vec![node_id],
                evidence_ids,
                ..SectionRefs::default()
            },
        );
    }
}

fn derive_business_domains(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    let mut domains = BTreeMap::<String, Vec<String>>::new();
    for route in &snapshot.routes {
        if let Some(domain) = route_domain(&route.url) {
            let evidence_id = builder.evidence(
                "route",
                &route.path,
                Some(route.handler_name.clone()),
                Some(route.line_range.clone()),
                Some(route.http_method.clone()),
                format!("{} {}", route.http_method, route.url),
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    for flag in &snapshot.feature_flags {
        if let Some(domain) = domain_token(&flag.name) {
            let evidence_id = builder.evidence(
                "feature_flag",
                &flag.path,
                Some(flag.name.clone()),
                Some(flag.line_range.clone()),
                Some(flag.edge_kind.clone()),
                format!("feature flag {}", flag.source_key),
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    for file in &snapshot.files {
        if let Some(domain) = path_domain(&file.path) {
            let evidence_id = builder.evidence(
                "path",
                &file.path,
                None,
                None,
                None,
                "domain-like path segment",
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    let mut ordered = domains.into_iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    if ordered.len() > builder.limit {
        builder.mark_node_budget_truncated();
    }
    for (domain, evidence_ids) in ordered.into_iter().take(builder.limit) {
        let node_id = builder.node(
            format!("domain:{domain}"),
            domain.clone(),
            "business_domain",
            None,
            domain_confidence(evidence_ids.len()),
            evidence_ids.first().cloned(),
        );
        builder.section(
            format!("section:domain:{domain}"),
            format!("{domain} domain"),
            format!(
                "{domain} is a candidate business domain from {} route, feature flag, or path signal(s).",
                evidence_ids.len()
            ),
            domain_confidence(evidence_ids.len()),
            SectionRefs {
                node_ids: node_id.into_iter().collect(),
                evidence_ids,
                ..SectionRefs::default()
            },
        );
    }
}

fn resolved_indexed_import_target<'a>(
    import: &'a CodeImportRecord,
    indexed_paths: &BTreeSet<&str>,
) -> Option<&'a str> {
    let target_path = import.target_hint.as_deref()?;
    (import.resolution_state == "resolved" && indexed_paths.contains(target_path))
        .then_some(target_path)
}

fn derive_process_flow(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    for route in snapshot.routes.iter().take(builder.limit) {
        let route_node_key = format!("route:{}", route.route_id);
        let handler_node_key = route_handler_node_id(route);
        let required_nodes =
            usize::from(builder.existing_node_id(route_node_key.clone()).is_none())
                + usize::from(builder.existing_node_id(handler_node_key.clone()).is_none());
        if !builder.can_insert_nodes(required_nodes) {
            builder.mark_node_budget_truncated();
            break;
        }
        let route_evidence = builder.evidence(
            "route",
            &route.path,
            Some(route.handler_name.clone()),
            Some(route.line_range.clone()),
            Some(route.http_method.clone()),
            format!("{} {}", route.http_method, route.url),
        );
        let route_id = builder.node(
            route_node_key,
            format!("{} {}", route.http_method.to_uppercase(), route.url),
            "route",
            Some(route.path.clone()),
            0.86,
            Some(route_evidence.clone()),
        );
        let Some(route_id) = route_id else {
            break;
        };
        let handler_id = builder.node(
            handler_node_key,
            route.handler_name.clone(),
            "handler",
            Some(route.path.clone()),
            0.82,
            Some(route_evidence.clone()),
        );
        let Some(handler_id) = handler_id else {
            break;
        };
        let mut edge_ids = Vec::new();
        if let Some(edge_id) = builder.edge(
            &route_id,
            &handler_id,
            "handled_by",
            0.86,
            Some(route_evidence.clone()),
        ) {
            edge_ids.push(edge_id);
        }
        let mut node_ids = vec![route_id, handler_id.clone()];
        let mut evidence_ids = vec![route_evidence];
        let matching_calls = snapshot
            .calls
            .iter()
            .filter(|call| call.call.path == route.path)
            .filter(|call| call_matches_route_handler(call, route))
            .collect::<Vec<_>>();
        let mut diagnostics = Vec::new();
        if matching_calls.len() > PROCESS_FLOW_CALL_LIMIT {
            builder.mark_edge_budget_truncated();
            diagnostics.push(format!(
                "route handler calls truncated to {PROCESS_FLOW_CALL_LIMIT} matching calls"
            ));
        }
        for call in matching_calls.into_iter().take(PROCESS_FLOW_CALL_LIMIT) {
            let call_evidence = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                format!("handler call to {}", call.call.callee_name),
            );
            let callee_id = builder.node(
                call_target_node_id(call),
                call.call.callee_name.clone(),
                "call_target",
                call.callee_path.clone(),
                0.68,
                Some(call_evidence.clone()),
            );
            if let Some(callee_id) = callee_id {
                if let Some(edge_id) = builder.edge(
                    &handler_id,
                    &callee_id,
                    "calls",
                    0.68,
                    Some(call_evidence.clone()),
                ) {
                    edge_ids.push(edge_id);
                    node_ids.push(callee_id);
                }
            }
            evidence_ids.push(call_evidence);
        }
        builder.section(
            format!("section:route:{}", route.route_id),
            format!("{} {}", route.http_method.to_uppercase(), route.url),
            format!(
                "Request flow starts at route {} {} and reaches handler {}.",
                route.http_method.to_uppercase(),
                route.url,
                route.handler_name
            ),
            0.78,
            SectionRefs {
                node_ids,
                edge_ids,
                evidence_ids,
                diagnostics,
            },
        );
    }
}

fn call_target_node_id(call: &CodebaseViewCall) -> String {
    if let Some(symbol_id) = call.call.callee_symbol_snapshot_id.as_deref() {
        return format!("call_target:symbol:{symbol_id}");
    }
    if let Some(path) = call.callee_path.as_deref() {
        return format!("call_target:path:{path}:{}", call.call.callee_name);
    }
    format!("call_target:{}:{}", call.call.path, call.call.callee_name)
}

fn route_handler_node_id(route: &CodeRouteRecord) -> String {
    route
        .handler_symbol_snapshot_id
        .as_ref()
        .map(|symbol_id| format!("handler:symbol:{symbol_id}"))
        .unwrap_or_else(|| format!("handler:{}:{}", route.path, route.route_id))
}

fn call_matches_route_handler(call: &CodebaseViewCall, route: &CodeRouteRecord) -> bool {
    if let (Some(caller_symbol_id), Some(handler_symbol_id)) = (
        call.call.caller_symbol_snapshot_id.as_deref(),
        route.handler_symbol_snapshot_id.as_deref(),
    ) {
        return caller_symbol_id == handler_symbol_id;
    }
    let Some(caller_name) = call.call.caller_name.as_deref() else {
        return false;
    };
    same_symbol_leaf(caller_name, &route.handler_name)
}

fn same_symbol_leaf(left: &str, right: &str) -> bool {
    left == right || symbol_leaf(left) == right || symbol_leaf(right) == left
}

fn symbol_leaf(name: &str) -> &str {
    name.rsplit([':', '.', '#', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(name)
}

fn derive_affected_scope(
    builder: &mut ViewBuilder,
    request: &CodebaseViewRequest,
    snapshot: &CodebaseViewSnapshot,
) {
    if request.changed_paths.is_empty() {
        let diagnostic =
            "affected_scope requires one or more --changed-path values in deterministic v1"
                .to_owned();
        builder.diagnostic(diagnostic.clone());
        builder.section(
            "section:affected_scope:missing_changes".to_owned(),
            "Affected scope needs changed paths".to_owned(),
            "No affected scope was derived because changed paths were not provided.".to_owned(),
            0.0,
            SectionRefs {
                diagnostics: vec![diagnostic],
                ..SectionRefs::default()
            },
        );
        return;
    }
    let changed_paths = normalized_view_paths(&request.changed_paths);
    if changed_paths.is_empty() {
        let diagnostic =
            "affected_scope requires one or more --changed-path values in deterministic v1"
                .to_owned();
        builder.diagnostic(diagnostic.clone());
        builder.section(
            "section:affected_scope:missing_changes".to_owned(),
            "Affected scope needs changed paths".to_owned(),
            "No affected scope was derived because changed paths were not provided.".to_owned(),
            0.0,
            SectionRefs {
                diagnostics: vec![diagnostic],
                ..SectionRefs::default()
            },
        );
        return;
    }
    let changed = changed_paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut node_ids = Vec::new();
    let mut edge_ids = Vec::new();
    let mut evidence_ids = Vec::new();
    for path in &changed_paths {
        let evidence_id = builder.evidence("changed_path", path, None, None, None, "changed input");
        let node_id = builder.node(
            format!("file:{path}"),
            path.clone(),
            "changed_file",
            Some(path.clone()),
            0.90,
            Some(evidence_id.clone()),
        );
        if let Some(node_id) = node_id {
            node_ids.push(node_id);
        }
        evidence_ids.push(evidence_id);
    }
    for call in &snapshot.calls {
        let touches_source = changed.contains(&call.call.path)
            || path_matches_changed_prefix(&call.call.path, &changed_paths);
        let touches_target = call.callee_path.as_ref().is_some_and(|path| {
            changed.contains(path) || path_matches_changed_prefix(path, &changed_paths)
        });
        if touches_source || touches_target {
            let target_path = call
                .callee_path
                .clone()
                .unwrap_or_else(|| call.call.path.clone());
            let evidence_id = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                format!("affected call to {}", call.call.callee_name),
            );
            let source_id = builder.node(
                format!("module:{}", module_key(&call.call.path)),
                module_key(&call.call.path),
                "affected_module",
                Some(call.call.path.clone()),
                0.70,
                Some(evidence_id.clone()),
            );
            let target_id = builder.node(
                format!("module:{}", module_key(&target_path)),
                module_key(&target_path),
                "affected_module",
                Some(target_path),
                0.70,
                Some(evidence_id.clone()),
            );
            if let (Some(source_id), Some(target_id)) = (&source_id, &target_id) {
                if let Some(edge_id) = builder.edge(
                    source_id,
                    target_id,
                    "affected_call",
                    0.70,
                    Some(evidence_id),
                ) {
                    edge_ids.push(edge_id);
                }
            }
            node_ids.extend([source_id, target_id].into_iter().flatten());
        }
    }
    for file in snapshot
        .files
        .iter()
        .filter(|file| is_test_config_or_doc(&file.path))
        .filter(|file| {
            changed_paths.iter().any(|changed_path| {
                affected_candidate_matches_changed_path(changed_path, &file.path)
            })
        })
        .take(builder.limit)
    {
        let evidence_id = builder.evidence(
            "candidate",
            &file.path,
            None,
            None,
            None,
            "test, configuration, or documentation candidate in changed module",
        );
        let node_id = builder.node(
            format!("candidate:{}", file.path),
            file.path.clone(),
            "verification_candidate",
            Some(file.path.clone()),
            0.62,
            Some(evidence_id.clone()),
        );
        if let Some(node_id) = node_id {
            node_ids.push(node_id);
        }
        evidence_ids.push(evidence_id);
    }
    node_ids.sort();
    node_ids.dedup();
    builder.section(
        "section:affected_scope".to_owned(),
        "Affected scope".to_owned(),
        format!(
            "Affected scope was derived from {} changed path(s), call edges, and nearby verification candidates.",
            changed_paths.len()
        ),
        0.68,
        SectionRefs {
            node_ids,
            edge_ids,
            evidence_ids,
            ..SectionRefs::default()
        },
    );
}

fn path_matches_changed_prefix(path: &str, changed_paths: &[String]) -> bool {
    changed_paths.iter().any(|changed_path| {
        path.strip_prefix(changed_path)
            .is_some_and(|tail| tail.starts_with('/'))
    })
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

fn empty_view_response(
    context: &RequestContext,
    graph_version: u64,
    status: &CodeRepositoryStatus,
    request: CodebaseViewRequest,
    degraded_reason: String,
) -> CodebaseViewResponse {
    CodebaseViewResponse {
        metadata: ApiMetadata::graph_only(context, crate::domain::GraphVersion::new(graph_version)),
        scope: crate::api::CodeRepositoryScopeMetadata::from_status(
            status,
            &request.repository,
            request.repository.ref_selector.clone(),
        ),
        freshness: CodeRepositoryFreshnessDiagnostics::graph_only(
            graph_version,
            request.freshness_policy,
            indexed_source_scope(status),
            request.repository.ref_selector.clone(),
            degraded_reason.clone(),
        ),
        request,
        graph_version,
        nodes: Vec::new(),
        edges: Vec::new(),
        sections: Vec::new(),
        evidence: Vec::new(),
        budget: CodebaseViewBudget::new(0, 0, false),
        diagnostics: vec![degraded_reason.clone()],
        degraded_reason: Some(degraded_reason),
    }
}

fn view_source_read_paths(request: &CodebaseViewRequest, derived: &DerivedView) -> Vec<String> {
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

fn served_scope_is_stale(status: &CodeRepositoryStatus, stale_reason: &Option<String>) -> bool {
    status.stale || stale_reason.is_some()
}

#[cfg(test)]
#[path = "views_dependency_tour_tests.rs"]
mod dependency_tour_tests;
#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;
