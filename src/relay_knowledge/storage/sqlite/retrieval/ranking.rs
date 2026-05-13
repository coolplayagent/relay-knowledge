use std::collections::BTreeMap;

use crate::domain::{RECIPROCAL_RANK_FUSION_K, RankingSignal, RetrievalHit, RetrieverSource};

use super::ScoredHit;

pub(super) fn merge_ranked(
    candidates: &mut BTreeMap<String, Candidate>,
    hits: Vec<ScoredHit>,
    fallback_source: RetrieverSource,
    explanation: &'static str,
) {
    for (index, scored) in hits.into_iter().enumerate() {
        let rank = index + 1;
        let rrf_score = 1.0 / (RECIPROCAL_RANK_FUSION_K + rank as f64);
        let source = match scored.source {
            RetrieverSource::CodeGraph => RetrieverSource::CodeGraph,
            _ => fallback_source,
        };
        let explanation_text = scored
            .explanation
            .unwrap_or_else(|| format!("{explanation}; modality={}", scored.modality));
        let candidate = candidates
            .entry(scored.key)
            .and_modify(|candidate| candidate.merge_hit(&scored.hit))
            .or_insert_with(|| Candidate::new(scored.hit));
        if !candidate.hit.retriever_sources.contains(&source) {
            candidate.hit.retriever_sources.push(source);
        }
        candidate.hit.ranking.push(RankingSignal {
            source,
            rank,
            score: scored.source_score,
            explanation: explanation_text,
        });
        candidate.rrf_score += rrf_score;
    }
}

pub(super) struct Candidate {
    hit: RetrievalHit,
    rrf_score: f64,
}

impl Candidate {
    fn new(hit: RetrievalHit) -> Self {
        Self {
            hit,
            rrf_score: 0.0,
        }
    }

    pub(super) fn into_hit(mut self) -> RetrievalHit {
        self.hit.score = self.rrf_score;
        self.hit
    }

    fn merge_hit(&mut self, hit: &RetrievalHit) {
        if !hit.content.is_empty() && !self.hit.content.contains(&hit.content) {
            if !self.hit.content.is_empty() {
                self.hit.content.push_str("\n\n");
            }
            self.hit.content.push_str(&hit.content);
        }
        if self.hit.source_path.is_none() {
            self.hit.source_path = hit.source_path.clone();
        }
        if self.hit.source_span.is_none() {
            self.hit.source_span = hit.source_span;
        }
        if self.hit.code_artifact.is_none() {
            self.hit.code_artifact = hit.code_artifact.clone();
        }
        for label in &hit.entity_labels {
            if !self.hit.entity_labels.contains(label) {
                self.hit.entity_labels.push(label.clone());
            }
        }
        for entity in &hit.entities {
            if !self
                .hit
                .entities
                .iter()
                .any(|existing| existing.id == entity.id)
            {
                self.hit.entities.push(entity.clone());
            }
        }
        for fact in &hit.graph_facts {
            if !self
                .hit
                .graph_facts
                .iter()
                .any(|existing| existing.fact_id == fact.fact_id && existing.kind == fact.kind)
            {
                self.hit.graph_facts.push(fact.clone());
            }
        }
    }
}
