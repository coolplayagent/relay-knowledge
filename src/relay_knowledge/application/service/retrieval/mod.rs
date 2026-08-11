//! Hybrid retrieval application workflow and response budgeting.

use std::sync::Arc;

use serde::Serialize;

use crate::{
    api::{ApiError, ApiMetadata, HybridRetrievalRequest, HybridRetrievalResponse, RequestContext},
    domain::{
        ContextGraphPath, ContextPackItem, FreshnessPolicy, FusionDiagnostics,
        RECIPROCAL_RANK_FUSION_K, RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit,
        RetrievalMode, RetrievedContextPack, RetrieverSource, SourceScope,
    },
    retrieval::{RetrievalPlan, read_model_backend_statuses},
    storage::{GraphSearchRequest, IndexRefreshDiagnostics, KnowledgeStore, StorageError},
};

use super::{
    super::knowledge::index_refresh::{
        IndexRefreshOutcome, metadata_for_indexes, refresh_index_kinds,
    },
    RelayKnowledgeService, current_time_millis, storage_api_error,
};

impl RelayKnowledgeService {
    /// Retrieves context through the unified hybrid retrieval contract.
    pub async fn retrieve_context(
        &self,
        request: HybridRetrievalRequest,
        context: RequestContext,
    ) -> Result<HybridRetrievalResponse, ApiError> {
        let source_scope = normalize_optional_source_scope(request.source_scope)
            .map_err(ApiError::invalid_argument)?;
        let plan = RetrievalPlan::new(
            request.query,
            source_scope,
            request.limit,
            request.freshness,
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        let mut retrieval_mode = RetrievalMode::Hybrid;
        let mut indexes = Vec::new();
        let mut index_cursors = Vec::new();
        let mut index_refresh = IndexRefreshDiagnostics::default();
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        let mut degraded_reasons = Vec::new();
        let backend_statuses = if plan.freshness == FreshnessPolicy::GraphOnly {
            retrieval_mode = RetrievalMode::GraphOnly;
            degraded_reasons.push("graph_only freshness policy selected".to_owned());
            Vec::new()
        } else {
            let mut index_outcome = retrieval_index_freshness_snapshot(&store).await?;
            indexes = index_outcome.indexes;
            index_cursors = index_outcome.cursors;
            index_refresh = index_outcome.diagnostics;
            let mut active_indexes = indexes
                .iter()
                .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
                .cloned()
                .collect::<Vec<_>>();
            if plan.freshness == FreshnessPolicy::WaitUntilFresh {
                let stale_kinds = active_indexes
                    .iter()
                    .filter(|status| status.is_stale_for(graph_version))
                    .map(|status| status.kind)
                    .collect::<Vec<_>>();
                if !stale_kinds.is_empty() {
                    refresh_index_kinds(
                        &store,
                        stale_kinds,
                        graph_version,
                        &self.runtime.retrieval,
                    )
                    .await?;
                    index_outcome = retrieval_index_freshness_snapshot(&store).await?;
                    indexes = index_outcome.indexes;
                    index_cursors = index_outcome.cursors;
                    index_refresh = index_outcome.diagnostics;
                    active_indexes = indexes
                        .iter()
                        .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
                        .cloned()
                        .collect();
                }
            }

            let stale = active_indexes
                .iter()
                .any(|status| status.is_stale_for(graph_version));
            metadata = metadata_for_indexes(&context, graph_version, &active_indexes);
            if plan.freshness == FreshnessPolicy::AllowStale && stale {
                degraded_reasons
                    .push("one or more indexes are behind the graph version".to_owned());
            }
            read_model_backend_statuses(&plan, graph_version, &indexes, &self.runtime.retrieval)
        };
        if backend_statuses
            .iter()
            .any(|status| status.state == crate::domain::RetrievalBackendState::Unavailable)
        {
            degraded_reasons.push(
                "semantic/vector retrieval backends unavailable; using bm25, graph evidence, and code graph fallback"
                    .to_owned(),
            );
        }
        let mut disabled_retriever_sources = self.runtime.retrieval.disabled_retriever_sources();
        if plan.freshness == FreshnessPolicy::GraphOnly {
            for source in [RetrieverSource::Semantic, RetrieverSource::Vector] {
                if !disabled_retriever_sources.contains(&source) {
                    disabled_retriever_sources.push(source);
                }
            }
        }
        let candidate_limit = self.runtime.retrieval.rerank.candidate_limit(plan.limit);
        let search_outcome = store
            .search(GraphSearchRequest {
                query: plan.query.clone(),
                source_scope: plan.source_scope.clone(),
                graph_version,
                limit: candidate_limit,
                disabled_retriever_sources,
            })
            .await
            .map_err(storage_api_error)?;
        if let Some(reason) = search_outcome.trace.degraded_reason.clone() {
            degraded_reasons.push(reason);
        }
        let (mut results, mut rerank) = self
            .runtime
            .retrieval
            .rerank
            .rerank(&plan.query, search_outcome.hits);
        let result_truncated = results.len() > plan.limit;
        results.truncate(plan.limit);
        rerank.returned_count = results.len();
        if rerank.degraded {
            if let Some(reason) = &rerank.reason {
                degraded_reasons.push(reason.clone());
            }
        }
        let degraded_reason = (!degraded_reasons.is_empty()).then(|| degraded_reasons.join("; "));
        let mut provenance_trace = search_outcome.trace;
        provenance_trace.mark_citations_for_hits(results.iter());
        provenance_trace.stale = degraded_reasons
            .iter()
            .any(|reason| reason.contains("behind the graph version"));
        provenance_trace.degraded_reason = degraded_reason.clone();
        provenance_trace.truncated |= result_truncated;
        provenance_trace.apply_budget(plan.limit.saturating_mul(4).max(plan.limit + 8).min(64));
        let truncated = result_truncated || provenance_trace.truncated;

        let context_pack = RetrievedContextPack {
            graph_version,
            source_scope: plan.source_scope.clone(),
            freshness: plan.freshness,
            truncated,
            backend_statuses: backend_statuses.clone(),
            provenance_trace: Some(provenance_trace),
            items: results
                .iter()
                .map(|hit| ContextPackItem {
                    result_id: hit.evidence_id.clone(),
                    source_scope: hit.source_scope.clone(),
                    source_path: hit.source_path.clone(),
                    source_span: hit.source_span,
                    entities: hit.entities.clone(),
                    graph_facts: hit.graph_facts.clone(),
                    graph_paths: hit
                        .graph_facts
                        .iter()
                        .map(ContextGraphPath::from_fact)
                        .collect(),
                    code_artifact: hit.code_artifact.clone(),
                    retriever_sources: hit.retriever_sources.clone(),
                    ranking: hit.ranking.clone(),
                    rerank: hit.rerank.clone(),
                })
                .collect(),
        };
        let budget_used = RetrievalBudgetUsed {
            limit: plan.limit,
            candidate_count: rerank.candidate_count,
            returned_count: results.len(),
            context_bytes: retrieval_context_bytes(&results, &context_pack, &backend_statuses),
        };
        let fusion = FusionDiagnostics {
            algorithm: "reciprocal_rank_fusion".to_owned(),
            k: RECIPROCAL_RANK_FUSION_K,
            candidate_count: budget_used.candidate_count,
        };

        Ok(HybridRetrievalResponse {
            metadata,
            context_pack,
            retrieval_mode,
            source_scope: plan.source_scope,
            freshness: plan.freshness,
            results,
            fusion,
            rerank,
            backend_statuses,
            truncated,
            budget_used,
            degraded_reason,
            indexes,
            index_cursors,
            index_refresh,
        })
    }
}

async fn retrieval_index_freshness_snapshot(
    store: &Arc<dyn KnowledgeStore>,
) -> Result<IndexRefreshOutcome, ApiError> {
    let indexes = store.index_statuses().await.map_err(storage_api_error)?;
    let cursors = match store.index_cursors().await {
        Ok(cursors) => cursors,
        Err(StorageError::InvalidInput(message))
            if message == "index cursor storage is unavailable" =>
        {
            Vec::new()
        }
        Err(error) => return Err(storage_api_error(error)),
    };
    let diagnostics = match store.index_refresh_diagnostics(current_time_millis()).await {
        Ok(diagnostics) => diagnostics,
        Err(StorageError::InvalidInput(message))
            if message == "index refresh diagnostics are unavailable" =>
        {
            IndexRefreshDiagnostics::default()
        }
        Err(error) => return Err(storage_api_error(error)),
    };

    Ok(IndexRefreshOutcome {
        indexes,
        cursors,
        diagnostics,
    })
}

fn normalize_optional_source_scope(value: Option<String>) -> Result<Option<String>, String> {
    value
        .map(|scope| {
            SourceScope::parse(scope)
                .map(String::from)
                .map_err(|error| error.to_string())
        })
        .transpose()
}

fn retrieval_context_bytes(
    results: &[RetrievalHit],
    context_pack: &RetrievedContextPack,
    backend_statuses: &[RetrievalBackendStatus],
) -> usize {
    serialized_context_bytes(&context_pack.backend_statuses)
        .saturating_add(serialized_context_bytes(backend_statuses))
        .saturating_add(
            context_pack
                .provenance_trace
                .as_ref()
                .map(serialized_context_bytes)
                .unwrap_or_default(),
        )
        .saturating_add(results.iter().map(serialized_context_bytes).sum::<usize>())
        .saturating_add(
            context_pack
                .items
                .iter()
                .map(serialized_context_bytes)
                .sum::<usize>(),
        )
}

fn serialized_context_bytes<T: Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX / 4)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
