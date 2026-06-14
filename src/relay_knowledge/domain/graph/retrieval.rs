use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use super::{ConfidenceScore, EvidenceSpan, FactStatus, GraphVersion, GraphVersionRange};

/// RRF constant used by Phase 1 hybrid retrieval.
pub const RECIPROCAL_RANK_FUSION_K: f64 = 60.0;

/// Freshness policy for hybrid retrieval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessPolicy {
    #[default]
    AllowStale,
    WaitUntilFresh,
    GraphOnly,
}

/// Retrieval path used to satisfy a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Hybrid,
    GraphOnly,
}

/// Retrieval source that contributed to a fused context result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrieverSource {
    Bm25,
    GraphEvidence,
    CodeGraph,
    Semantic,
    Vector,
    GraphPath,
    Temporal,
    CommunitySummary,
}

/// Rerank backend requested for the hybrid retrieval candidate set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerankMode {
    Local,
    External,
    Disabled,
}

impl RerankMode {
    /// Parses a stable environment/config value.
    pub fn parse(value: &str) -> Result<Self, RerankModeError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "external" => Ok(Self::External),
            "disabled" => Ok(Self::Disabled),
            other => Err(RerankModeError {
                value: other.to_owned(),
            }),
        }
    }

    /// Stable configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::External => "external",
            Self::Disabled => "disabled",
        }
    }
}

/// Invalid rerank backend mode supplied by runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankModeError {
    pub value: String,
}

impl fmt::Display for RerankModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "rerank backend '{}' must be local, external, or disabled",
            self.value
        )
    }
}

impl Error for RerankModeError {}

/// Availability state for optional retrieval backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalBackendState {
    Available,
    Degraded,
    Unavailable,
}

/// Per-backend status preserved so callers can distinguish fallback from
/// complete hybrid retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBackendStatus {
    pub source: RetrieverSource,
    pub state: RetrievalBackendState,
    pub scope_post_filter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_graph_version: Option<GraphVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RetrieverSource {
    /// Stable API representation used in ranking diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::GraphEvidence => "graph_evidence",
            Self::CodeGraph => "code_graph",
            Self::Semantic => "semantic",
            Self::Vector => "vector",
            Self::GraphPath => "graph_path",
            Self::Temporal => "temporal",
            Self::CommunitySummary => "community_summary",
        }
    }
}

#[cfg(test)]
#[path = "retrieval_tests.rs"]
mod tests;

/// Per-retriever ranking signal preserved after fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingSignal {
    pub source: RetrieverSource,
    pub rank: usize,
    pub score: f64,
    pub explanation: String,
}

/// Final rerank signal applied after hybrid retrieval fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankSignal {
    pub mode: RerankMode,
    pub score: f64,
    pub explanation: String,
}

/// Budget actually consumed by retrieval context packing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBudgetUsed {
    pub limit: usize,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub context_bytes: usize,
}

/// Diagnostics for reciprocal-rank fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusionDiagnostics {
    pub algorithm: String,
    pub k: f64,
    pub candidate_count: usize,
}

/// Diagnostics for post-fusion reranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankDiagnostics {
    pub requested_mode: RerankMode,
    pub effective_mode: RerankMode,
    pub algorithm: String,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A compact, auditable context pack for agent and UI adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedContextPack {
    pub graph_version: GraphVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub freshness: FreshnessPolicy,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_statuses: Vec<RetrievalBackendStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_trace: Option<TraversalProvenanceTrace>,
    pub items: Vec<ContextPackItem>,
}

/// Bounded explanation of the graph traversal and candidate path used for an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraversalProvenanceTrace {
    pub graph_version: GraphVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub routed_intent: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visited_nodes: Vec<TraversalTraceNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visited_edges: Vec<TraversalTraceEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cited_evidence: Vec<TraversalTraceEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visited_but_uncited: Vec<TraversalTraceEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranking_contributions: Vec<TraversalRankingContribution>,
    pub truncated: bool,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub redaction: TraversalTraceRedaction,
}

/// Node reached while building a retrieval context pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalTraceNode {
    pub node_id: String,
    pub label: String,
    pub kind: TraversalTraceNodeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_ids: Vec<String>,
}

/// Stable node categories exposed in traversal provenance traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraversalTraceNodeKind {
    Entity,
    Evidence,
    CodeArtifact,
}

/// Edge reached while building a retrieval context pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalTraceEdge {
    pub edge_id: String,
    pub from_node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    pub source: RetrieverSource,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Evidence candidate reached during retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraversalTraceEvidence {
    pub evidence_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub score: f64,
    pub retriever_sources: Vec<RetrieverSource>,
}

/// Per-source ranking contribution retained before final context truncation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraversalRankingContribution {
    pub result_id: String,
    pub source: RetrieverSource,
    pub rank: usize,
    pub score: f64,
    pub rrf_contribution: f64,
    pub cited: bool,
    pub explanation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Authorization and budget redaction summary for a traversal trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalTraceRedaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_scope: Option<String>,
    pub redacted_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Entity projection retained with each context item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEntity {
    pub id: String,
    pub label: String,
}

/// Structured graph fact kind referenced from a context item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextGraphFactKind {
    Relation,
    Claim,
    Event,
}

impl ContextGraphFactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Relation => "relation",
            Self::Claim => "claim",
            Self::Event => "event",
        }
    }
}

/// Structured relation, claim, or event that supports a retrieval hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphFact {
    pub fact_id: String,
    pub kind: ContextGraphFactKind,
    pub subject: String,
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

/// Direct graph path evidence derived from a structured graph fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphPath {
    pub path_id: String,
    pub nodes: Vec<String>,
    pub edges: Vec<ContextGraphPathEdge>,
}

/// One edge in a graph path returned through the context pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphPathEdge {
    pub fact_id: String,
    pub kind: ContextGraphFactKind,
    pub from: String,
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

impl ContextGraphPath {
    /// Builds a one-hop path from a persisted structured fact.
    pub fn from_fact(fact: &ContextGraphFact) -> Self {
        let mut nodes = vec![fact.subject.clone()];
        if let Some(object) = &fact.object
            && !nodes.contains(object)
        {
            nodes.push(object.clone());
        }

        Self {
            path_id: format!("path:{}", fact.fact_id),
            nodes,
            edges: vec![ContextGraphPathEdge {
                fact_id: fact.fact_id.clone(),
                kind: fact.kind,
                from: fact.subject.clone(),
                predicate: fact.predicate.clone(),
                to: fact.object.clone(),
                evidence_ids: fact.evidence_ids.clone(),
                confidence: fact.confidence,
                status: fact.status,
                version_range: fact.version_range,
            }],
        }
    }
}

/// Code artifact category returned through the general GraphRAG context pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphArtifactKind {
    Symbol,
    Chunk,
}

/// Code graph artifact tied to a shared retrieval result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphArtifact {
    pub kind: CodeGraphArtifactKind,
    pub artifact_id: String,
    pub path: String,
}

/// Context-pack item tied to a retrieval hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPackItem {
    pub result_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<EvidenceSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ContextEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_facts: Vec<ContextGraphFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_paths: Vec<ContextGraphPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_artifact: Option<CodeGraphArtifact>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankSignal>,
}

/// A context item returned by retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub evidence_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<EvidenceSpan>,
    pub content: String,
    pub entity_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ContextEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_facts: Vec<ContextGraphFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_artifact: Option<CodeGraphArtifact>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankSignal>,
    pub score: f64,
}

impl TraversalProvenanceTrace {
    /// Builds a traversal trace from storage candidates before answer-level citation is known.
    pub fn from_hits(
        graph_version: GraphVersion,
        source_scope: Option<String>,
        routed_intent: String,
        hits: &[RetrievalHit],
    ) -> Self {
        let mut trace = Self {
            graph_version,
            source_scope: source_scope.clone(),
            routed_intent,
            visited_nodes: Vec::new(),
            visited_edges: Vec::new(),
            cited_evidence: Vec::new(),
            visited_but_uncited: Vec::new(),
            ranking_contributions: Vec::new(),
            truncated: false,
            stale: false,
            degraded_reason: None,
            redaction: TraversalTraceRedaction {
                authorization_scope: source_scope,
                redacted_count: 0,
                reason: None,
            },
        };

        for hit in hits {
            if !trace.trace_scope_allows(&hit.source_scope) {
                trace.redaction.redacted_count += 1;
                trace.redaction.reason = Some("source_scope authorization filter".to_owned());
                continue;
            }
            trace.push_evidence(hit);
            trace.push_hit_nodes(hit);
            trace.push_hit_edges(hit);
            trace.push_code_artifact_edge(hit);
            trace.push_ranking_contributions(hit);
        }

        trace
    }

    /// Marks which visited evidence items are cited by the final context pack.
    pub fn mark_citations<I>(&mut self, cited_result_ids: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let cited_ids = cited_result_ids
            .into_iter()
            .map(|id| id.as_ref().to_owned())
            .collect::<std::collections::BTreeSet<_>>();

        let visited_evidence = self.all_visited_evidence();
        self.cited_evidence.clear();
        self.visited_but_uncited.clear();
        for contribution in &mut self.ranking_contributions {
            contribution.cited = cited_ids.contains(contribution.result_id.as_str());
        }
        for evidence in visited_evidence {
            if cited_ids.contains(evidence.evidence_id.as_str()) {
                self.cited_evidence.push(evidence);
            } else {
                self.visited_but_uncited.push(evidence);
            }
        }
    }

    pub(crate) fn mark_citations_for_hits<'a, I>(&mut self, cited_hits: I)
    where
        I: IntoIterator<Item = &'a RetrievalHit>,
    {
        let cited_keys = cited_hits
            .into_iter()
            .map(TraceEvidenceKey::from_hit)
            .collect::<std::collections::BTreeSet<_>>();
        let visited_evidence = self.all_visited_evidence();
        self.cited_evidence.clear();
        self.visited_but_uncited.clear();
        for contribution in &mut self.ranking_contributions {
            contribution.cited = trace_contribution_matches_keys(contribution, &cited_keys);
        }
        for evidence in visited_evidence {
            if cited_keys.contains(&TraceEvidenceKey::from_evidence(&evidence)) {
                self.cited_evidence.push(evidence);
            } else {
                self.visited_but_uncited.push(evidence);
            }
        }
    }

    pub(crate) fn retain_hits<'a, I>(&mut self, retained_hits: I)
    where
        I: IntoIterator<Item = &'a RetrievalHit>,
    {
        let retained_keys = retained_hits
            .into_iter()
            .map(TraceEvidenceKey::from_hit)
            .collect::<std::collections::BTreeSet<_>>();
        self.cited_evidence
            .retain(|evidence| retained_keys.contains(&TraceEvidenceKey::from_evidence(evidence)));
        self.visited_but_uncited
            .retain(|evidence| retained_keys.contains(&TraceEvidenceKey::from_evidence(evidence)));
        self.ranking_contributions
            .retain(|contribution| trace_contribution_matches_keys(contribution, &retained_keys));
        self.visited_edges
            .retain(|edge| trace_edge_matches_keys(edge, &retained_keys));
        let retained_edge_node_keys = self
            .visited_edges
            .iter()
            .flat_map(trace_edge_endpoint_keys)
            .collect::<std::collections::BTreeSet<_>>();
        self.visited_nodes.retain(|node| {
            retained_edge_node_keys.contains(&TraceNodeKey::from_node(node))
                || trace_node_matches_keys(node, &retained_keys)
        });
    }

    /// Truncates low-priority trace detail without dropping cited evidence first.
    pub fn apply_budget(&mut self, max_trace_items: usize) {
        let max_trace_items = max_trace_items.max(1);
        let cited_keys = self
            .cited_evidence
            .iter()
            .map(TraceEvidenceKey::from_evidence)
            .collect::<std::collections::BTreeSet<_>>();
        let cited_edge_node_keys = self
            .visited_edges
            .iter()
            .filter(|edge| trace_edge_matches_keys(edge, &cited_keys))
            .flat_map(trace_edge_endpoint_keys)
            .collect::<std::collections::BTreeSet<_>>();
        self.visited_nodes.sort_by(|left, right| {
            let left_cited = cited_edge_node_keys.contains(&TraceNodeKey::from_node(left))
                || trace_node_matches_keys(left, &cited_keys);
            let right_cited = cited_edge_node_keys.contains(&TraceNodeKey::from_node(right))
                || trace_node_matches_keys(right, &cited_keys);
            right_cited
                .cmp(&left_cited)
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.source_scope.cmp(&right.source_scope))
                .then_with(|| left.source_path.cmp(&right.source_path))
                .then_with(|| left.node_id.cmp(&right.node_id))
        });
        self.visited_nodes.dedup_by(|left, right| {
            left.node_id == right.node_id
                && left.kind == right.kind
                && left.source_scope == right.source_scope
                && left.source_path == right.source_path
                && left.evidence_ids == right.evidence_ids
        });
        self.visited_edges.sort_by(|left, right| {
            let left_cited = trace_edge_matches_keys(left, &cited_keys);
            let right_cited = trace_edge_matches_keys(right, &cited_keys);
            right_cited
                .cmp(&left_cited)
                .then_with(|| left.edge_id.cmp(&right.edge_id))
        });
        self.visited_edges
            .dedup_by(|left, right| left.edge_id == right.edge_id);
        self.visited_but_uncited.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.evidence_id.cmp(&right.evidence_id))
        });
        self.cited_evidence.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.evidence_id.cmp(&right.evidence_id))
        });
        self.ranking_contributions.sort_by(|left, right| {
            right
                .cited
                .cmp(&left.cited)
                .then_with(|| right.rrf_contribution.total_cmp(&left.rrf_contribution))
                .then_with(|| left.result_id.cmp(&right.result_id))
        });

        self.truncated |= truncate_vec(&mut self.visited_nodes, max_trace_items);
        self.truncated |= truncate_vec(&mut self.visited_edges, max_trace_items);
        self.truncated |= truncate_vec(&mut self.cited_evidence, max_trace_items);
        self.truncated |= truncate_vec(&mut self.visited_but_uncited, max_trace_items);
        self.truncated |= truncate_vec(&mut self.ranking_contributions, max_trace_items);
    }

    fn trace_scope_allows(&self, hit_scope: &str) -> bool {
        self.source_scope
            .as_deref()
            .is_none_or(|scope| scope == hit_scope)
    }

    fn push_evidence(&mut self, hit: &RetrievalHit) {
        let source_path = trace_source_path(hit);
        if self.visited_but_uncited.iter().any(|evidence| {
            evidence.evidence_id == hit.evidence_id
                && evidence.source_scope == hit.source_scope
                && evidence.source_path == source_path
        }) {
            return;
        }
        self.visited_but_uncited.push(TraversalTraceEvidence {
            evidence_id: hit.evidence_id.clone(),
            source_scope: hit.source_scope.clone(),
            source_path,
            score: hit.score,
            retriever_sources: hit.retriever_sources.clone(),
        });
    }

    fn push_hit_nodes(&mut self, hit: &RetrievalHit) {
        let source_path = trace_source_path(hit);
        self.visited_nodes.push(TraversalTraceNode {
            node_id: format!("evidence:{}", hit.evidence_id),
            label: hit.evidence_id.clone(),
            kind: TraversalTraceNodeKind::Evidence,
            source_scope: Some(hit.source_scope.clone()),
            source_path: source_path.clone(),
            evidence_ids: vec![hit.evidence_id.clone()],
        });
        if let Some(artifact) = &hit.code_artifact {
            self.visited_nodes.push(TraversalTraceNode {
                node_id: code_artifact_node_id(hit, artifact),
                label: artifact.artifact_id.clone(),
                kind: TraversalTraceNodeKind::CodeArtifact,
                source_scope: Some(hit.source_scope.clone()),
                source_path: trace_artifact_path(artifact),
                evidence_ids: vec![hit.evidence_id.clone()],
            });
        }
        for entity in &hit.entities {
            self.visited_nodes.push(TraversalTraceNode {
                node_id: entity.id.clone(),
                label: entity.label.clone(),
                kind: TraversalTraceNodeKind::Entity,
                source_scope: Some(hit.source_scope.clone()),
                source_path: source_path.clone(),
                evidence_ids: vec![hit.evidence_id.clone()],
            });
        }
        for fact in &hit.graph_facts {
            let evidence_ids = trace_edge_evidence_ids(hit, &fact.evidence_ids);
            self.visited_nodes.push(TraversalTraceNode {
                node_id: format!("entity-label:{}", fact.subject),
                label: fact.subject.clone(),
                kind: TraversalTraceNodeKind::Entity,
                source_scope: Some(hit.source_scope.clone()),
                source_path: source_path.clone(),
                evidence_ids: evidence_ids.clone(),
            });
            if let Some(object) = &fact.object {
                self.visited_nodes.push(TraversalTraceNode {
                    node_id: format!("entity-label:{object}"),
                    label: object.clone(),
                    kind: TraversalTraceNodeKind::Entity,
                    source_scope: Some(hit.source_scope.clone()),
                    source_path: source_path.clone(),
                    evidence_ids: evidence_ids.clone(),
                });
            }
        }
    }

    fn push_hit_edges(&mut self, hit: &RetrievalHit) {
        for fact in &hit.graph_facts {
            self.visited_edges.push(TraversalTraceEdge {
                edge_id: format!("{}:{}", fact.kind.as_str(), fact.fact_id),
                from_node_id: format!("entity-label:{}", fact.subject),
                to_node_id: fact
                    .object
                    .as_ref()
                    .map(|object| format!("entity-label:{object}")),
                predicate: Some(fact.predicate.clone()),
                source: trace_edge_source(hit),
                evidence_ids: trace_edge_evidence_ids(hit, &fact.evidence_ids),
                source_scope: Some(hit.source_scope.clone()),
                source_path: trace_source_path(hit),
            });
        }
    }

    fn push_code_artifact_edge(&mut self, hit: &RetrievalHit) {
        if let Some(artifact) = &hit.code_artifact {
            self.visited_edges.push(TraversalTraceEdge {
                edge_id: format!(
                    "code-artifact:{}:{}:{}:{}:{}",
                    hit.source_scope,
                    artifact.path,
                    hit.evidence_id,
                    artifact.kind.as_str(),
                    artifact.artifact_id
                ),
                from_node_id: format!("evidence:{}", hit.evidence_id),
                to_node_id: Some(code_artifact_node_id(hit, artifact)),
                predicate: Some("code_artifact".to_owned()),
                source: trace_edge_source(hit),
                evidence_ids: vec![hit.evidence_id.clone()],
                source_scope: Some(hit.source_scope.clone()),
                source_path: trace_artifact_path(artifact),
            });
        }
    }

    fn push_ranking_contributions(&mut self, hit: &RetrievalHit) {
        let source_path = trace_source_path(hit);
        for signal in &hit.ranking {
            self.ranking_contributions
                .push(TraversalRankingContribution {
                    result_id: hit.evidence_id.clone(),
                    source: signal.source,
                    rank: signal.rank,
                    score: signal.score,
                    rrf_contribution: 1.0 / (RECIPROCAL_RANK_FUSION_K + signal.rank as f64),
                    cited: false,
                    explanation: signal.explanation.clone(),
                    source_scope: Some(hit.source_scope.clone()),
                    source_path: source_path.clone(),
                });
        }
    }

    fn all_visited_evidence(&self) -> Vec<TraversalTraceEvidence> {
        let mut evidence = self
            .cited_evidence
            .iter()
            .chain(self.visited_but_uncited.iter())
            .cloned()
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            left.evidence_id
                .cmp(&right.evidence_id)
                .then_with(|| left.source_scope.cmp(&right.source_scope))
                .then_with(|| left.source_path.cmp(&right.source_path))
        });
        evidence.dedup_by(|left, right| {
            left.evidence_id == right.evidence_id
                && left.source_scope == right.source_scope
                && left.source_path == right.source_path
        });
        evidence
    }
}

impl CodeGraphArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Chunk => "chunk",
        }
    }
}

fn truncate_vec<T>(items: &mut Vec<T>, max: usize) -> bool {
    if items.len() <= max {
        return false;
    }
    items.truncate(max);
    true
}

fn trace_edge_evidence_ids(hit: &RetrievalHit, fact_evidence_ids: &[String]) -> Vec<String> {
    let mut evidence_ids = fact_evidence_ids.to_vec();
    if !evidence_ids.contains(&hit.evidence_id) {
        evidence_ids.push(hit.evidence_id.clone());
    }
    evidence_ids
}

fn trace_edge_source(hit: &RetrievalHit) -> RetrieverSource {
    if hit.retriever_sources.contains(&RetrieverSource::GraphPath) {
        return RetrieverSource::GraphPath;
    }
    hit.retriever_sources
        .first()
        .copied()
        .or_else(|| hit.ranking.first().map(|signal| signal.source))
        .unwrap_or(RetrieverSource::GraphEvidence)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TraceEvidenceKey {
    evidence_id: String,
    source_scope: String,
    source_path: Option<String>,
}

impl TraceEvidenceKey {
    fn from_hit(hit: &RetrievalHit) -> Self {
        Self {
            evidence_id: hit.evidence_id.clone(),
            source_scope: hit.source_scope.clone(),
            source_path: trace_source_path(hit),
        }
    }

    fn from_evidence(evidence: &TraversalTraceEvidence) -> Self {
        Self {
            evidence_id: evidence.evidence_id.clone(),
            source_scope: evidence.source_scope.clone(),
            source_path: evidence.source_path.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TraceNodeKey {
    node_id: String,
    source_scope: Option<String>,
    source_path: Option<String>,
}

impl TraceNodeKey {
    fn from_node(node: &TraversalTraceNode) -> Self {
        Self {
            node_id: node.node_id.clone(),
            source_scope: node.source_scope.clone(),
            source_path: node.source_path.clone(),
        }
    }

    fn from_edge_node(edge: &TraversalTraceEdge, node_id: &str) -> Self {
        Self {
            node_id: node_id.to_owned(),
            source_scope: edge.source_scope.clone(),
            source_path: edge.source_path.clone(),
        }
    }
}

fn trace_edge_endpoint_keys(edge: &TraversalTraceEdge) -> impl Iterator<Item = TraceNodeKey> + '_ {
    std::iter::once(TraceNodeKey::from_edge_node(edge, &edge.from_node_id)).chain(
        edge.to_node_id
            .iter()
            .map(|node_id| TraceNodeKey::from_edge_node(edge, node_id)),
    )
}

fn trace_source_path(hit: &RetrievalHit) -> Option<String> {
    hit.source_path
        .clone()
        .or_else(|| hit.code_artifact.as_ref().and_then(trace_artifact_path))
}

fn trace_artifact_path(artifact: &CodeGraphArtifact) -> Option<String> {
    (!artifact.path.is_empty()).then(|| artifact.path.clone())
}

fn trace_node_matches_keys(
    node: &TraversalTraceNode,
    keys: &std::collections::BTreeSet<TraceEvidenceKey>,
) -> bool {
    keys.iter().any(|key| {
        node.source_scope.as_deref() == Some(key.source_scope.as_str())
            && node.source_path == key.source_path
            && (node
                .evidence_ids
                .iter()
                .any(|evidence_id| evidence_id == &key.evidence_id)
                || node
                    .node_id
                    .strip_prefix("evidence:")
                    .is_some_and(|id| id == key.evidence_id))
    })
}

fn trace_edge_matches_keys(
    edge: &TraversalTraceEdge,
    keys: &std::collections::BTreeSet<TraceEvidenceKey>,
) -> bool {
    keys.iter().any(|key| {
        edge.source_scope.as_deref() == Some(key.source_scope.as_str())
            && edge.source_path == key.source_path
            && edge
                .evidence_ids
                .iter()
                .any(|evidence_id| evidence_id == &key.evidence_id)
    })
}

fn trace_contribution_matches_keys(
    contribution: &TraversalRankingContribution,
    keys: &std::collections::BTreeSet<TraceEvidenceKey>,
) -> bool {
    keys.iter().any(|key| {
        contribution.result_id == key.evidence_id
            && contribution.source_scope.as_deref() == Some(key.source_scope.as_str())
            && contribution.source_path == key.source_path
    })
}

fn code_artifact_node_id(hit: &RetrievalHit, artifact: &CodeGraphArtifact) -> String {
    format!(
        "code:{}:{}:{}:{}",
        hit.source_scope,
        artifact.path,
        artifact.kind.as_str(),
        artifact.artifact_id
    )
}
