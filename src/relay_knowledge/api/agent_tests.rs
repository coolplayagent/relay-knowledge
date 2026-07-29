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
