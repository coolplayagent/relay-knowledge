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
#[path = "agent_tests.rs"]
mod tests;
