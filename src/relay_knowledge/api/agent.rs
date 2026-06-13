use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::domain::{
    ContextPackItem, FreshnessPolicy, FusionDiagnostics, IndexStatus, RetrievalBackendStatus,
    RetrievalHit, RetrievalMode, RetrievedContextPack,
};
use crate::project::{ACP_LOCAL_ADAPTER_NAME, MCP_ADAPTER_NAME};
use crate::storage::{IndexCursor, IndexRefreshDiagnostics};

use super::{ApiMetadata, RequestContext};

/// Agent protocol family used by external resident-process adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentProtocolKind {
    Mcp,
    Acp,
}

/// Runtime identity captured from an agent protocol request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    pub protocol: AgentProtocolKind,
    pub adapter_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl RuntimeIdentity {
    /// Creates the resident MCP adapter identity for a single request.
    pub fn mcp(tool_call_id: Option<String>) -> Self {
        Self {
            protocol: AgentProtocolKind::Mcp,
            adapter_name: MCP_ADAPTER_NAME.to_owned(),
            adapter_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            client_name: None,
            client_version: None,
            host_name: None,
            actor_id: None,
            session_id: None,
            tool_call_id,
        }
    }

    /// Creates the local ACP adapter identity for one session request.
    pub fn acp(
        client_name: Option<String>,
        client_version: Option<String>,
        actor_id: Option<String>,
        session_id: String,
        request_id: Option<String>,
    ) -> Self {
        Self {
            protocol: AgentProtocolKind::Acp,
            adapter_name: ACP_LOCAL_ADAPTER_NAME.to_owned(),
            adapter_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            client_name,
            client_version,
            host_name: None,
            actor_id,
            session_id: Some(session_id),
            tool_call_id: request_id,
        }
    }
}

/// Unified API context plus agent protocol identity and policy provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequestContext {
    pub request: RequestContext,
    pub runtime_identity: RuntimeIdentity,
    pub policy_id: String,
}

/// Local access policy applied before agent protocol requests reach services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAccessPolicy {
    pub allowed_scopes: Vec<String>,
    pub allow_unspecified_scope: bool,
    pub max_limit: usize,
    pub max_context_bytes: usize,
    pub max_runtime_ms: u64,
    pub allow_remote_clients: bool,
}

impl AgentAccessPolicy {
    pub const DEFAULT_MAX_LIMIT: usize = 10;
    pub const DEFAULT_MAX_CONTEXT_BYTES: usize = 65_536;

    /// Creates a validated access policy for agent protocol adapters.
    pub fn new(
        allowed_scopes: Vec<String>,
        allow_unspecified_scope: bool,
        max_limit: usize,
        max_context_bytes: usize,
        max_runtime_ms: u64,
        allow_remote_clients: bool,
    ) -> Result<Self, AgentPolicyError> {
        if max_limit == 0 {
            return Err(AgentPolicyError::ZeroMaxLimit);
        }
        if max_context_bytes == 0 {
            return Err(AgentPolicyError::ZeroMaxContextBytes);
        }
        if max_runtime_ms == 0 {
            return Err(AgentPolicyError::ZeroMaxRuntime);
        }

        Ok(Self {
            allowed_scopes,
            allow_unspecified_scope,
            max_limit,
            max_context_bytes,
            max_runtime_ms,
            allow_remote_clients,
        })
    }

    /// Summarizes policy without exposing scope names or secrets.
    pub fn summary(&self) -> AgentAccessPolicySummary {
        AgentAccessPolicySummary {
            allowed_scope_count: self.allowed_scopes.len(),
            allow_unspecified_scope: self.allow_unspecified_scope,
            max_limit: self.max_limit,
            max_context_bytes: self.max_context_bytes,
            max_runtime_ms: self.max_runtime_ms,
            allow_remote_clients: self.allow_remote_clients,
        }
    }
}

/// Stable policy validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentPolicyError {
    ZeroMaxLimit,
    ZeroMaxContextBytes,
    ZeroMaxRuntime,
}

impl std::fmt::Display for AgentPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroMaxLimit => write!(formatter, "MCP max limit must be greater than zero"),
            Self::ZeroMaxContextBytes => {
                write!(formatter, "MCP max context bytes must be greater than zero")
            }
            Self::ZeroMaxRuntime => write!(formatter, "MCP max runtime must be greater than zero"),
        }
    }
}

impl std::error::Error for AgentPolicyError {}

/// Redacted policy status for service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAccessPolicySummary {
    pub allowed_scope_count: usize,
    pub allow_unspecified_scope: bool,
    pub max_limit: usize,
    pub max_context_bytes: usize,
    pub max_runtime_ms: u64,
    pub allow_remote_clients: bool,
}

/// Service status projection for resident agent protocols.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProtocolStatus {
    pub mcp_streamable_http_enabled: bool,
    pub mcp_endpoint: String,
    pub mcp_resources_enabled: bool,
    pub mcp_prompts_enabled: bool,
    pub metrics_endpoint: String,
    pub http_bind: String,
    pub allowed_origin_count: usize,
    pub mcp_allowed_origins: Vec<String>,
    pub policy: AgentAccessPolicySummary,
    pub audit_sink_enabled: bool,
    pub audit_log_path: String,
    pub audit_queue_depth: usize,
}

/// Canonical retrieval result shared by MCP and future agent protocols.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRetrievalResult {
    pub metadata: ApiMetadata,
    pub runtime_identity: RuntimeIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub freshness: String,
    pub retrieval_mode: RetrievalMode,
    pub context_pack: RetrievedContextPack,
    pub results: Vec<RetrievalHit>,
    pub fusion: FusionDiagnostics,
    pub rerank: crate::domain::RerankDiagnostics,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_statuses: Vec<RetrievalBackendStatus>,
    pub indexes: Vec<IndexStatus>,
    #[serde(default)]
    pub index_cursors: Vec<IndexCursor>,
    #[serde(default)]
    pub index_refresh: IndexRefreshDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub truncated: bool,
    pub budget_used: AgentBudgetUsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AgentResultKey {
    result_id: String,
    source_scope: String,
    source_path: Option<String>,
}

impl AgentResultKey {
    fn from_hit(hit: &RetrievalHit) -> Self {
        Self {
            result_id: hit.evidence_id.clone(),
            source_scope: hit.source_scope.clone(),
            source_path: agent_hit_source_path(hit),
        }
    }

    fn from_item(item: &ContextPackItem) -> Self {
        Self {
            result_id: item.result_id.clone(),
            source_scope: item.source_scope.clone(),
            source_path: item
                .source_path
                .clone()
                .or_else(|| item.code_artifact.as_ref().and_then(agent_artifact_path)),
        }
    }
}

fn agent_hit_source_path(hit: &RetrievalHit) -> Option<String> {
    hit.source_path
        .clone()
        .or_else(|| hit.code_artifact.as_ref().and_then(agent_artifact_path))
}

fn agent_artifact_path(artifact: &crate::domain::CodeGraphArtifact) -> Option<String> {
    (!artifact.path.is_empty()).then(|| artifact.path.clone())
}

impl AgentRetrievalResult {
    /// Builds the canonical agent result and applies the context byte budget.
    pub fn from_retrieval(
        response: crate::api::HybridRetrievalResponse,
        identity: RuntimeIdentity,
        max_context_bytes: usize,
        elapsed_ms: u64,
    ) -> Self {
        let crate::api::HybridRetrievalResponse {
            metadata,
            mut context_pack,
            retrieval_mode,
            source_scope,
            freshness,
            results: response_results,
            fusion,
            mut rerank,
            mut backend_statuses,
            truncated: response_truncated,
            budget_used,
            degraded_reason,
            indexes,
            index_cursors,
            index_refresh,
        } = response;
        let item_bytes = context_pack
            .items
            .iter()
            .map(|item| {
                (
                    AgentResultKey::from_item(item),
                    serialized_context_bytes(item),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut context_bytes = serialized_context_bytes(&context_pack.backend_statuses)
            .saturating_add(serialized_context_bytes(&backend_statuses));
        let mut truncated = response_truncated;
        if context_bytes > max_context_bytes {
            context_pack.backend_statuses.clear();
            backend_statuses.clear();
            context_bytes = 0;
            truncated = true;
        }
        let mut results = Vec::new();

        for hit in response_results {
            let hit_key = AgentResultKey::from_hit(&hit);
            let hit_bytes = serialized_context_bytes(&hit)
                .saturating_add(item_bytes.get(&hit_key).copied().unwrap_or_default());
            if context_bytes.saturating_add(hit_bytes) > max_context_bytes {
                truncated = true;
                continue;
            }
            context_bytes += hit_bytes;
            results.push(hit);
        }
        let returned_count = results.len();
        rerank.returned_count = returned_count;
        let retained_result_keys = results
            .iter()
            .map(AgentResultKey::from_hit)
            .collect::<HashSet<_>>();
        context_pack.truncated = truncated;
        context_pack
            .items
            .retain(|item| retained_result_keys.contains(&AgentResultKey::from_item(item)));
        if let Some(trace) = &mut context_pack.provenance_trace {
            trace.retain_hits(results.iter());
            trace.mark_citations_for_hits(results.iter());
            trace.truncated |= truncated;
            trace.apply_budget(
                returned_count
                    .saturating_mul(4)
                    .max(returned_count + 8)
                    .min(64),
            );
            if trace.truncated {
                truncated = true;
                context_pack.truncated = true;
            }
        }
        if let Some(trace) = &mut context_pack.provenance_trace {
            let mut trace_bytes = serialized_context_bytes(trace);
            if context_bytes.saturating_add(trace_bytes) > max_context_bytes {
                trace.apply_budget(returned_count.max(1));
                trace.truncated = true;
                truncated = true;
                context_pack.truncated = true;
                trace_bytes = serialized_context_bytes(trace);
            }
            if context_bytes.saturating_add(trace_bytes) > max_context_bytes {
                context_pack.provenance_trace = None;
                truncated = true;
                context_pack.truncated = true;
            } else {
                context_bytes += trace_bytes;
            }
        }

        Self {
            metadata,
            runtime_identity: identity,
            source_scope,
            freshness: freshness_label(freshness).to_owned(),
            retrieval_mode,
            context_pack,
            results,
            fusion,
            rerank,
            backend_statuses,
            indexes,
            index_cursors,
            index_refresh,
            degraded_reason,
            truncated,
            budget_used: AgentBudgetUsed {
                limit: budget_used.limit,
                candidate_count: budget_used.candidate_count,
                returned_count,
                context_bytes,
                elapsed_ms,
            },
        }
    }
}

fn serialized_context_bytes<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX / 4)
}

/// Runtime budget consumed by a completed agent retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBudgetUsed {
    pub limit: usize,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub context_bytes: usize,
    pub elapsed_ms: u64,
}

pub fn freshness_label(freshness: FreshnessPolicy) -> &'static str {
    match freshness {
        FreshnessPolicy::AllowStale => "allow-stale",
        FreshnessPolicy::WaitUntilFresh => "wait-until-fresh",
        FreshnessPolicy::GraphOnly => "graph-only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::InterfaceKind,
        domain::{
            ConfidenceScore, ContextGraphFact, ContextGraphFactKind, ContextPackItem, FactStatus,
            FusionDiagnostics, GraphVersion, GraphVersionRange, RerankDiagnostics, RerankMode,
            RetrievalBackendState, RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit,
            RetrievedContextPack, RetrieverSource, TraversalProvenanceTrace,
        },
    };

    #[test]
    fn truncates_retrieval_results_to_context_byte_budget() {
        let items = vec![pack_item("ev-1"), pack_item("ev-2"), pack_item("ev-3")];
        let results = vec![
            hit("ev-1", "abcd"),
            hit("ev-2", "efgh"),
            hit("ev-3", "ijkl"),
        ];
        let max_context_bytes = serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&results[0])
            + serialized_context_bytes(&items[0])
            + serialized_context_bytes(&results[1])
            + serialized_context_bytes(&items[1]);
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: Vec::new(),
                provenance_trace: None,
                items,
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results,
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 3,
            },
            rerank: rerank_diagnostics(3, 3),
            backend_statuses: Vec::new(),
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 3,
                candidate_count: 3,
                returned_count: 3,
                context_bytes: 12,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics {
                queue_depth: 2,
                ..IndexRefreshDiagnostics::default()
            },
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            max_context_bytes,
            4,
        );

        assert!(result.truncated);
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.context_pack.items.len(), 2);
        assert_eq!(result.budget_used.returned_count, 2);
        assert_eq!(result.rerank.returned_count, 2);
        assert_eq!(result.budget_used.context_bytes, max_context_bytes);
        assert_eq!(result.freshness, "allow-stale");
        assert_eq!(result.index_refresh.queue_depth, 2);
    }

    #[test]
    fn omits_backend_metadata_when_it_exceeds_agent_context_budget() {
        let backend_statuses = vec![RetrievalBackendStatus {
            source: RetrieverSource::Semantic,
            state: RetrievalBackendState::Unavailable,
            scope_post_filter: true,
            indexed_graph_version: Some(GraphVersion::new(1)),
            reason: Some("semantic backend disabled by local policy".repeat(8)),
        }];
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: backend_statuses.clone(),
                provenance_trace: None,
                items: Vec::new(),
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results: Vec::new(),
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 0,
            },
            rerank: rerank_diagnostics(0, 0),
            backend_statuses,
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 3,
                candidate_count: 0,
                returned_count: 0,
                context_bytes: 0,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            8,
            4,
        );

        assert!(result.truncated);
        assert!(result.backend_statuses.is_empty());
        assert!(result.context_pack.backend_statuses.is_empty());
        assert!(result.budget_used.context_bytes <= 8);
    }

    #[test]
    fn omits_trace_before_dropping_cited_results_when_context_budget_is_tight() {
        let results = vec![hit("ev-1", "grounded answer content")];
        let items = vec![pack_item("ev-1")];
        let mut trace = TraversalProvenanceTrace::from_hits(
            GraphVersion::new(1),
            Some("docs".to_owned()),
            "direct_context_lookup".to_owned(),
            &results,
        );
        trace.mark_citations(["ev-1"]);
        let max_context_bytes = serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&results[0])
            + serialized_context_bytes(&items[0]);
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: Vec::new(),
                provenance_trace: Some(trace),
                items,
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results,
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 1,
            },
            rerank: rerank_diagnostics(1, 1),
            backend_statuses: Vec::new(),
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 1,
                candidate_count: 1,
                returned_count: 1,
                context_bytes: 0,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            max_context_bytes,
            4,
        );

        assert!(result.truncated);
        assert_eq!(result.results.len(), 1);
        assert!(result.context_pack.provenance_trace.is_none());
    }

    #[test]
    fn reports_truncated_agent_result_when_trace_is_budgeted_but_retained() {
        let mut result_hit = hit("ev-1", "grounded answer content");
        result_hit.graph_facts = (0..16)
            .map(|index| graph_fact(index, "ev-1"))
            .collect::<Vec<_>>();
        result_hit.retriever_sources = vec![RetrieverSource::GraphPath];
        let results = vec![result_hit];
        let items = vec![pack_item("ev-1")];
        let mut trace = TraversalProvenanceTrace::from_hits(
            GraphVersion::new(1),
            Some("docs".to_owned()),
            "direct_context_lookup".to_owned(),
            &results,
        );
        trace.mark_citations(["ev-1"]);
        let mut budgeted_trace = trace.clone();
        budgeted_trace.apply_budget(9);
        budgeted_trace.apply_budget(1);
        budgeted_trace.truncated = true;
        let max_context_bytes = serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&results[0])
            + serialized_context_bytes(&items[0])
            + serialized_context_bytes(&budgeted_trace);
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: Vec::new(),
                provenance_trace: Some(trace),
                items,
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results,
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 1,
            },
            rerank: rerank_diagnostics(1, 1),
            backend_statuses: Vec::new(),
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 1,
                candidate_count: 1,
                returned_count: 1,
                context_bytes: 0,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            max_context_bytes,
            4,
        );

        assert!(result.truncated);
        assert!(result.context_pack.truncated);
        assert!(
            result
                .context_pack
                .provenance_trace
                .as_ref()
                .is_some_and(|trace| trace.truncated)
        );
    }

    #[test]
    fn reports_truncated_agent_result_when_trace_items_are_budgeted() {
        let mut result_hit = hit("ev-1", "grounded answer content");
        result_hit.graph_facts = (0..16)
            .map(|index| graph_fact(index, "ev-1"))
            .collect::<Vec<_>>();
        result_hit.retriever_sources = vec![RetrieverSource::GraphPath];
        let results = vec![result_hit];
        let items = vec![pack_item("ev-1")];
        let mut trace = TraversalProvenanceTrace::from_hits(
            GraphVersion::new(1),
            Some("docs".to_owned()),
            "direct_context_lookup".to_owned(),
            &results,
        );
        trace.mark_citations(["ev-1"]);
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: Vec::new(),
                provenance_trace: Some(trace),
                items,
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results,
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 1,
            },
            rerank: rerank_diagnostics(1, 1),
            backend_statuses: Vec::new(),
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 1,
                candidate_count: 1,
                returned_count: 1,
                context_bytes: 0,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            usize::MAX,
            4,
        );

        assert!(result.truncated);
        assert!(result.context_pack.truncated);
        assert!(
            result
                .context_pack
                .provenance_trace
                .as_ref()
                .is_some_and(|trace| trace.truncated)
        );
    }

    #[test]
    fn filters_dropped_hits_from_agent_trace_before_byte_budget() {
        let retained_hit = hit("ev-1", "grounded answer content");
        let mut dropped_hit = hit("ev-2", "omitted answer content");
        dropped_hit.graph_facts = (0..32)
            .map(|index| graph_fact(index, "ev-2"))
            .collect::<Vec<_>>();
        dropped_hit.retriever_sources = vec![RetrieverSource::GraphPath];
        let results = vec![retained_hit, dropped_hit];
        let items = vec![pack_item("ev-1"), pack_item("ev-2")];
        let mut trace = TraversalProvenanceTrace::from_hits(
            GraphVersion::new(1),
            Some("docs".to_owned()),
            "direct_context_lookup".to_owned(),
            &results,
        );
        trace.mark_citations(["ev-1", "ev-2"]);
        let mut retained_trace = trace.clone();
        retained_trace.retain_hits([&results[0]]);
        retained_trace.mark_citations_for_hits([&results[0]]);
        retained_trace.truncated = true;
        retained_trace.apply_budget(9);
        let max_context_bytes = serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&Vec::<RetrievalBackendStatus>::new())
            + serialized_context_bytes(&results[0])
            + serialized_context_bytes(&items[0])
            + serialized_context_bytes(&retained_trace);
        let response = crate::api::HybridRetrievalResponse {
            metadata: ApiMetadata {
                trace_id: "trace".to_owned(),
                request_id: "req".to_owned(),
                graph_version: 1,
                index_version: None,
                indexed_graph_version: None,
                stale: false,
            },
            context_pack: RetrievedContextPack {
                graph_version: GraphVersion::new(1),
                source_scope: Some("docs".to_owned()),
                freshness: FreshnessPolicy::AllowStale,
                truncated: false,
                backend_statuses: Vec::new(),
                provenance_trace: Some(trace),
                items,
            },
            retrieval_mode: RetrievalMode::Hybrid,
            source_scope: Some("docs".to_owned()),
            freshness: FreshnessPolicy::AllowStale,
            results,
            fusion: FusionDiagnostics {
                algorithm: "reciprocal_rank_fusion".to_owned(),
                k: 60.0,
                candidate_count: 2,
            },
            rerank: rerank_diagnostics(2, 2),
            backend_statuses: Vec::new(),
            truncated: false,
            budget_used: RetrievalBudgetUsed {
                limit: 2,
                candidate_count: 2,
                returned_count: 2,
                context_bytes: 0,
            },
            degraded_reason: None,
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
        };

        let result = AgentRetrievalResult::from_retrieval(
            response,
            RuntimeIdentity::mcp(Some("call-1".to_owned())),
            max_context_bytes,
            4,
        );

        assert!(result.truncated);
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].evidence_id, "ev-1");
        let trace = result
            .context_pack
            .provenance_trace
            .as_ref()
            .expect("retained-only trace should fit");
        assert!(
            trace
                .cited_evidence
                .iter()
                .all(|evidence| evidence.evidence_id == "ev-1")
        );
        assert!(
            trace
                .ranking_contributions
                .iter()
                .all(|contribution| contribution.result_id == "ev-1")
        );
    }

    #[test]
    fn rejects_zero_policy_budgets() {
        let error = AgentAccessPolicy::new(Vec::new(), false, 0, 1, 1, false).expect_err("zero");

        assert_eq!(error, AgentPolicyError::ZeroMaxLimit);
    }

    fn hit(evidence_id: &str, content: &str) -> RetrievalHit {
        RetrievalHit {
            evidence_id: evidence_id.to_owned(),
            source_scope: "docs".to_owned(),
            source_path: None,
            source_span: None,
            content: content.to_owned(),
            entity_labels: Vec::new(),
            entities: Vec::new(),
            graph_facts: Vec::new(),
            code_artifact: None,
            retriever_sources: Vec::new(),
            ranking: Vec::new(),
            rerank: None,
            score: 1.0,
        }
    }

    fn pack_item(result_id: &str) -> ContextPackItem {
        ContextPackItem {
            result_id: result_id.to_owned(),
            source_scope: "docs".to_owned(),
            source_path: None,
            source_span: None,
            entities: Vec::new(),
            graph_facts: Vec::new(),
            graph_paths: Vec::new(),
            code_artifact: None,
            retriever_sources: Vec::new(),
            ranking: Vec::new(),
            rerank: None,
        }
    }

    fn graph_fact(index: usize, evidence_id: &str) -> ContextGraphFact {
        ContextGraphFact {
            fact_id: format!("fact-{index}"),
            kind: ContextGraphFactKind::Relation,
            subject: format!("source-{index}"),
            predicate: "supports".to_owned(),
            object: Some(format!("target-{index}")),
            evidence_ids: vec![evidence_id.to_owned()],
            confidence: ConfidenceScore { basis_points: 9000 },
            status: FactStatus::Accepted,
            version_range: GraphVersionRange::open_from(GraphVersion::new(1)),
        }
    }

    fn rerank_diagnostics(candidate_count: usize, returned_count: usize) -> RerankDiagnostics {
        RerankDiagnostics {
            requested_mode: RerankMode::Local,
            effective_mode: RerankMode::Local,
            algorithm: "deterministic_feature_rerank".to_owned(),
            candidate_count,
            returned_count,
            degraded: false,
            reason: None,
        }
    }

    #[test]
    fn carries_agent_context_without_domain_identity_leakage() {
        let context = AgentRequestContext {
            request: RequestContext::with_ids(InterfaceKind::Mcp, "req", "trace"),
            runtime_identity: RuntimeIdentity::mcp(Some("tool".to_owned())),
            policy_id: "default".to_owned(),
        };

        assert_eq!(context.request.interface, InterfaceKind::Mcp);
        assert_eq!(context.runtime_identity.protocol, AgentProtocolKind::Mcp);
    }
}
