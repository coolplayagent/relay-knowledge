use std::{
    collections::{BTreeSet, HashMap, HashSet},
    time::Instant,
};

use crate::{
    api::{
        ApiError, CodeGraphContextResponse, CodeRepositoryFreshnessDiagnostics,
        CodeRepositoryFreshnessState, CodeRepositoryQueryResponse, RequestContext,
    },
    application::RelayKnowledgeService,
    domain::{
        CodeGraphCodeExcerpt, CodeGraphContextBudget, CodeGraphContextPack,
        CodeGraphContextProvenance, CodeGraphContextRequest, CodeGraphImpactHint, CodeQueryKind,
        CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
    },
};

const ENTRY_QUERY_KINDS: [CodeQueryKind; 3] = [
    CodeQueryKind::Hybrid,
    CodeQueryKind::Definition,
    CodeQueryKind::Symbol,
];
const EXPANSION_QUERY_KINDS: [CodeQueryKind; 4] = [
    CodeQueryKind::References,
    CodeQueryKind::Callers,
    CodeQueryKind::Callees,
    CodeQueryKind::Imports,
];
const MAX_CONTEXT_SEEDS: usize = 3;
const MAX_EXPANSION_LIMIT: usize = 4;

impl RelayKnowledgeService {
    /// Builds an agent-oriented codegraph context pack with bounded graph expansion.
    pub async fn codegraph_context(
        &self,
        request: CodeGraphContextRequest,
        context: RequestContext,
    ) -> Result<CodeGraphContextResponse, ApiError> {
        let started = Instant::now();
        let mut candidate_count = 0usize;
        let mut entry_points = Vec::new();
        let mut freshness_parts = Vec::new();
        let mut context_request = request.clone();
        let mut primary = None;

        for kind in ENTRY_QUERY_KINDS {
            let response = self
                .run_context_query(
                    &context_request,
                    kind,
                    request.query.clone(),
                    request.limit,
                    &context,
                    None,
                )
                .await?;
            candidate_count = candidate_count.saturating_add(response.results.len());
            push_unique_hits(&mut entry_points, response.results.clone());
            if kind == CodeQueryKind::Hybrid {
                context_request = pinned_context_request(&request, &response.scope);
                primary = Some(response.clone());
            } else {
                freshness_parts.push(response.freshness);
            }
        }

        let primary = primary.expect("hybrid entry query always runs");
        let expansion_constraints = context_query_constraints(&context_request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let seeds = context_seeds(&entry_points);
        let mut related_symbols = Vec::new();
        let mut graph_paths = Vec::new();
        let mut context_roles = HashMap::new();

        for seed in &seeds {
            let seed_query = seed_query(seed, &request.query);
            for kind in EXPANSION_QUERY_KINDS {
                let response = self
                    .run_context_query(
                        &context_request,
                        kind,
                        seed_query.clone(),
                        request.limit.min(MAX_EXPANSION_LIMIT),
                        &context,
                        Some(&expansion_constraints),
                    )
                    .await?;
                candidate_count = candidate_count.saturating_add(response.results.len());
                remember_context_roles(&mut context_roles, kind, &response.results);
                let results = response.results;
                if kind == CodeQueryKind::References {
                    push_unique_hits(&mut related_symbols, results);
                } else {
                    push_unique_hits(&mut graph_paths, results);
                }
                freshness_parts.push(response.freshness);
            }
        }

        let count_truncated = truncate_hits(&mut entry_points, request.limit)
            | truncate_hits(&mut related_symbols, request.limit)
            | truncate_hits(&mut graph_paths, request.limit);
        apply_code_visibility(&mut entry_points, request.include_code);
        apply_code_visibility(&mut related_symbols, request.include_code);
        apply_code_visibility(&mut graph_paths, request.include_code);

        let mut pack = CodeGraphContextPack {
            code_excerpts: code_excerpts(
                request.include_code,
                &entry_points,
                &related_symbols,
                &graph_paths,
                &context_roles,
            ),
            impact_hints: impact_hints(&graph_paths, &context_roles),
            entry_points,
            related_symbols,
            graph_paths,
        };
        let byte_truncated = pack_to_budget(&mut pack, request.max_context_bytes, &context_roles);
        let mut truncated = count_truncated | byte_truncated;
        let context_bytes = serialized_context_bytes(&pack);
        truncated |= primary.results.len() > request.limit;
        let retrieval_layers = retrieval_layers(&pack);
        let mut freshness = merge_context_freshness(primary.freshness, freshness_parts);
        freshness.merge_direct_source_read_paths(context_paths(&pack));
        let returned_count = pack.entry_points.len()
            + pack.related_symbols.len()
            + pack.graph_paths.len()
            + pack.code_excerpts.len();
        let mut diagnostics = vec![format!(
            "Expanded {} seed(s) through references, callers, callees, and imports with bounded limits.",
            seeds.len()
        )];
        if count_truncated {
            diagnostics.push("Context pack was truncated to fit the requested limit.".to_owned());
        }
        if byte_truncated {
            diagnostics.push("Context pack was truncated to fit max_context_bytes.".to_owned());
        }

        Ok(CodeGraphContextResponse {
            metadata: primary.metadata,
            query: request.query.clone(),
            repository_scope: primary.scope,
            freshness,
            budget: CodeGraphContextBudget {
                limit: request.limit,
                max_context_bytes: request.max_context_bytes,
                candidate_count,
                returned_count,
                context_bytes,
                elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            },
            truncated,
            retrieval_layers,
            request,
            pack,
            diagnostics,
        })
    }

    async fn run_context_query(
        &self,
        request: &CodeGraphContextRequest,
        kind: CodeQueryKind,
        query: String,
        limit: usize,
        context: &RequestContext,
        constraints: Option<&CodeRetrievalRequest>,
    ) -> Result<CodeRepositoryQueryResponse, ApiError> {
        let mut retrieval = CodeRetrievalRequest::new(
            query,
            request.repository.clone(),
            kind,
            limit,
            request.freshness_policy,
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        retrieval.exclude_generated = request.exclude_generated;
        if let Some(constraints) = constraints {
            carry_context_filters(&mut retrieval, constraints);
        }

        self.query_code_repository(retrieval, context.clone()).await
    }
}

fn context_query_constraints(
    request: &CodeGraphContextRequest,
) -> Result<CodeRetrievalRequest, crate::domain::DomainError> {
    CodeRetrievalRequest::new(
        request.query.clone(),
        request.repository.clone(),
        CodeQueryKind::Hybrid,
        request.limit,
        request.freshness_policy,
    )
}

fn carry_context_filters(target: &mut CodeRetrievalRequest, source: &CodeRetrievalRequest) {
    target.query_language_filters = source.query_language_filters.clone();
    target.query_path_substrings = source.query_path_substrings.clone();
}

fn pinned_context_request(
    original: &CodeGraphContextRequest,
    primary_scope: &crate::api::CodeRepositoryScopeMetadata,
) -> CodeGraphContextRequest {
    let mut pinned = original.clone();
    if !primary_scope.resolved_commit_sha.is_empty() {
        pinned.repository.ref_selector = primary_scope.resolved_commit_sha.clone();
    }
    pinned
}

fn context_seeds(entry_points: &[CodeRetrievalHit]) -> Vec<CodeRetrievalHit> {
    let mut seeds = entry_points.to_vec();
    seeds.sort_by(|left, right| right.score.total_cmp(&left.score));
    seeds.truncate(MAX_CONTEXT_SEEDS);
    seeds
}

fn push_unique_hits(target: &mut Vec<CodeRetrievalHit>, hits: Vec<CodeRetrievalHit>) {
    let mut keys = target.iter().map(hit_key).collect::<HashSet<_>>();
    for hit in hits {
        if keys.insert(hit_key(&hit)) {
            target.push(hit);
        }
    }
    target.sort_by(|left, right| right.score.total_cmp(&left.score));
}

fn truncate_hits(hits: &mut Vec<CodeRetrievalHit>, limit: usize) -> bool {
    if hits.len() > limit {
        hits.truncate(limit);
        true
    } else {
        false
    }
}

fn remember_context_roles(
    roles: &mut HashMap<String, CodeQueryKind>,
    kind: CodeQueryKind,
    hits: &[CodeRetrievalHit],
) {
    for hit in hits {
        roles.entry(hit_key(hit)).or_insert(kind);
    }
}

fn hit_key(hit: &CodeRetrievalHit) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        hit.path,
        hit.line_range.start,
        hit.line_range.end,
        hit.symbol_snapshot_id.as_deref().unwrap_or(""),
        hit.edge_kind.as_deref().unwrap_or("")
    )
}

fn seed_query(hit: &CodeRetrievalHit, fallback: &str) -> String {
    hit.canonical_symbol_id
        .as_deref()
        .and_then(extract_searchable_symbol_tail)
        .unwrap_or(fallback)
        .to_owned()
}

fn extract_searchable_symbol_tail(value: &str) -> Option<&str> {
    value
        .rsplit([':', '/', '#', '.', '@', '[', ']'])
        .find(|part| part.chars().any(|character| character.is_alphanumeric()))
}

fn apply_code_visibility(hits: &mut [CodeRetrievalHit], include_code: bool) {
    if include_code {
        return;
    }
    for hit in hits {
        hit.excerpt.clear();
    }
}

fn code_excerpts(
    include_code: bool,
    entry_points: &[CodeRetrievalHit],
    related_symbols: &[CodeRetrievalHit],
    graph_paths: &[CodeRetrievalHit],
    context_roles: &HashMap<String, CodeQueryKind>,
) -> Vec<CodeGraphCodeExcerpt> {
    if !include_code {
        return Vec::new();
    }
    entry_points
        .iter()
        .chain(related_symbols)
        .chain(graph_paths)
        .filter(|hit| !hit.excerpt.trim().is_empty())
        .map(|hit| CodeGraphCodeExcerpt {
            path: hit.path.clone(),
            language_id: hit.language_id.clone(),
            line_range: hit.line_range.clone(),
            symbol_snapshot_id: hit.symbol_snapshot_id.clone(),
            provenance: CodeGraphContextProvenance {
                query_kind: provenance_kind(hit, context_roles),
                retrieval_layers: hit.retrieval_layers.clone(),
                score: hit.score,
            },
            excerpt: hit.excerpt.clone(),
        })
        .collect()
}

fn impact_hints(
    graph_paths: &[CodeRetrievalHit],
    context_roles: &HashMap<String, CodeQueryKind>,
) -> Vec<CodeGraphImpactHint> {
    graph_paths
        .iter()
        .map(|hit| CodeGraphImpactHint {
            path: hit.path.clone(),
            line_range: hit.line_range.clone(),
            relationship: context_roles
                .get(&hit_key(hit))
                .map(context_role_relationship)
                .or(hit.edge_kind.as_deref())
                .unwrap_or_else(|| relationship_from_layers(&hit.retrieval_layers))
                .to_owned(),
            symbol_snapshot_id: hit.symbol_snapshot_id.clone(),
            retrieval_layers: hit.retrieval_layers.clone(),
            score: hit.score,
        })
        .collect()
}

fn provenance_kind(
    hit: &CodeRetrievalHit,
    context_roles: &HashMap<String, CodeQueryKind>,
) -> CodeQueryKind {
    if let Some(kind) = context_roles.get(&hit_key(hit)) {
        *kind
    } else if hit
        .retrieval_layers
        .contains(&CodeRetrievalLayer::Reference)
    {
        CodeQueryKind::References
    } else if hit
        .retrieval_layers
        .contains(&CodeRetrievalLayer::CallGraph)
    {
        CodeQueryKind::Callers
    } else if hit
        .retrieval_layers
        .contains(&CodeRetrievalLayer::ImportGraph)
    {
        CodeQueryKind::Imports
    } else if hit
        .retrieval_layers
        .contains(&CodeRetrievalLayer::Definition)
    {
        CodeQueryKind::Definition
    } else if hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol) {
        CodeQueryKind::Symbol
    } else {
        CodeQueryKind::Hybrid
    }
}

fn context_role_relationship(kind: &CodeQueryKind) -> &'static str {
    match kind {
        CodeQueryKind::References => "reference",
        CodeQueryKind::Callers => "caller",
        CodeQueryKind::Callees => "callee",
        CodeQueryKind::Imports => "import",
        _ => "context",
    }
}

fn relationship_from_layers(layers: &[CodeRetrievalLayer]) -> &'static str {
    if layers.contains(&CodeRetrievalLayer::CallGraph) {
        "call_graph"
    } else if layers.contains(&CodeRetrievalLayer::ImportGraph) {
        "import_graph"
    } else if layers.contains(&CodeRetrievalLayer::Reference) {
        "reference"
    } else {
        "related"
    }
}

fn merge_context_freshness(
    mut primary: CodeRepositoryFreshnessDiagnostics,
    parts: Vec<CodeRepositoryFreshnessDiagnostics>,
) -> CodeRepositoryFreshnessDiagnostics {
    for freshness in parts {
        primary.state = worse_freshness_state(primary.state, freshness.state);
        primary.scope_stale |= freshness.scope_stale;
        primary.direct_source_read_required |= freshness.direct_source_read_required;
        primary.index_lag.requested_ref_indexed &= freshness.index_lag.requested_ref_indexed;
        primary.index_lag.pending_task_count = primary
            .index_lag
            .pending_task_count
            .max(freshness.index_lag.pending_task_count);
        primary.index_lag.pending_file_count = max_optional_usize(
            primary.index_lag.pending_file_count,
            freshness.index_lag.pending_file_count,
        );
        primary.pending.active_for_repository |= freshness.pending.active_for_repository;
        primary.pending.active_matches_request |= freshness.pending.active_matches_request;
        primary.pending.queue_depth = primary
            .pending
            .queue_depth
            .max(freshness.pending.queue_depth);
        primary.pending.queued_task_count = primary
            .pending
            .queued_task_count
            .max(freshness.pending.queued_task_count);
        primary.pending.running_task_count = primary
            .pending
            .running_task_count
            .max(freshness.pending.running_task_count);
        primary.pending.retrying_task_count = primary
            .pending
            .retrying_task_count
            .max(freshness.pending.retrying_task_count);
        primary.pending.dead_letter_task_count = primary
            .pending
            .dead_letter_task_count
            .max(freshness.pending.dead_letter_task_count);
        primary.pending.running_lease_count = primary
            .pending
            .running_lease_count
            .max(freshness.pending.running_lease_count);
        primary.stale_reason = merge_reason(primary.stale_reason.take(), freshness.stale_reason);
        primary.degraded_reason =
            merge_reason(primary.degraded_reason.take(), freshness.degraded_reason);
        primary.merge_direct_source_read_paths(freshness.direct_source_read_paths);
    }
    primary
}

fn worse_freshness_state(
    left: CodeRepositoryFreshnessState,
    right: CodeRepositoryFreshnessState,
) -> CodeRepositoryFreshnessState {
    if freshness_state_rank(left) >= freshness_state_rank(right) {
        left
    } else {
        right
    }
}

fn freshness_state_rank(state: CodeRepositoryFreshnessState) -> u8 {
    match state {
        CodeRepositoryFreshnessState::Fresh => 0,
        CodeRepositoryFreshnessState::Degraded => 1,
        CodeRepositoryFreshnessState::Stale => 2,
        CodeRepositoryFreshnessState::Pending => 3,
    }
}

fn max_optional_usize(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn merge_reason(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(left), Some(right)) => Some(format!("{left}; {right}")),
        (Some(reason), None) | (None, Some(reason)) => Some(reason),
        (None, None) => None,
    }
}

fn pack_to_budget(
    pack: &mut CodeGraphContextPack,
    max_context_bytes: usize,
    context_roles: &HashMap<String, CodeQueryKind>,
) -> bool {
    let mut truncated = false;
    while serialized_context_bytes(pack) > max_context_bytes {
        let removed_code_excerpt = pack.code_excerpts.pop().is_some();
        let cleared_expansion_excerpts =
            !removed_code_excerpt && clear_expansion_hit_excerpts(pack);
        if removed_code_excerpt || cleared_expansion_excerpts {
            truncated = true;
        } else if pack.graph_paths.pop().is_some() {
            pack.impact_hints = impact_hints(&pack.graph_paths, context_roles);
            truncated = true;
        } else if pack.related_symbols.pop().is_some()
            || clear_entry_hit_excerpts(pack)
            || pack.entry_points.pop().is_some()
        {
            truncated = true;
        } else {
            clear_hit_excerpts(pack);
            return true;
        }
    }

    truncated
}

fn clear_hit_excerpts(pack: &mut CodeGraphContextPack) {
    clear_expansion_hit_excerpts(pack);
    clear_entry_hit_excerpts(pack);
    pack.code_excerpts.clear();
    pack.impact_hints.clear();
}

fn clear_expansion_hit_excerpts(pack: &mut CodeGraphContextPack) -> bool {
    let had_excerpts = pack
        .related_symbols
        .iter()
        .chain(&pack.graph_paths)
        .any(|hit| !hit.excerpt.is_empty());
    apply_code_visibility(&mut pack.related_symbols, false);
    apply_code_visibility(&mut pack.graph_paths, false);

    had_excerpts
}

fn clear_entry_hit_excerpts(pack: &mut CodeGraphContextPack) -> bool {
    let had_excerpts = pack.entry_points.iter().any(|hit| !hit.excerpt.is_empty());
    apply_code_visibility(&mut pack.entry_points, false);

    had_excerpts
}

fn retrieval_layers(pack: &CodeGraphContextPack) -> Vec<CodeRetrievalLayer> {
    let mut layers = BTreeSet::new();
    for hit in pack
        .entry_points
        .iter()
        .chain(&pack.related_symbols)
        .chain(&pack.graph_paths)
    {
        for layer in &hit.retrieval_layers {
            layers.insert(layer.as_str());
        }
    }

    layers
        .into_iter()
        .filter_map(layer_from_str)
        .collect::<Vec<_>>()
}

fn layer_from_str(value: &str) -> Option<CodeRetrievalLayer> {
    match value {
        "lexical" => Some(CodeRetrievalLayer::Lexical),
        "symbol" => Some(CodeRetrievalLayer::Symbol),
        "definition" => Some(CodeRetrievalLayer::Definition),
        "reference" => Some(CodeRetrievalLayer::Reference),
        "call_graph" => Some(CodeRetrievalLayer::CallGraph),
        "import_graph" => Some(CodeRetrievalLayer::ImportGraph),
        "sbom" => Some(CodeRetrievalLayer::Sbom),
        "impact" => Some(CodeRetrievalLayer::Impact),
        "text_fallback" => Some(CodeRetrievalLayer::TextFallback),
        _ => None,
    }
}

fn serialized_context_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX / 4)
}

fn context_paths(pack: &CodeGraphContextPack) -> Vec<String> {
    pack.entry_points
        .iter()
        .chain(&pack.related_symbols)
        .chain(&pack.graph_paths)
        .map(|hit| hit.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
