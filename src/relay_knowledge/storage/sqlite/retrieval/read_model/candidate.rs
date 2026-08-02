use crate::domain::{RetrievalHit, RetrieverSource};

pub(in crate::storage::sqlite::retrieval) struct ScoredHit {
    pub(in crate::storage::sqlite::retrieval) key: String,
    pub(in crate::storage::sqlite::retrieval) hit: RetrievalHit,
    pub(in crate::storage::sqlite::retrieval) source: RetrieverSource,
    pub(in crate::storage::sqlite::retrieval) source_score: f64,
    pub(in crate::storage::sqlite::retrieval) modality: String,
    pub(in crate::storage::sqlite::retrieval) explanation: Option<String>,
}

pub(in crate::storage::sqlite::retrieval) fn evidence_group_key(evidence_id: &str) -> String {
    format!("evidence_group:{evidence_id}")
}

pub(in crate::storage::sqlite::retrieval) fn sort_scored_hits(hits: &mut [ScoredHit]) {
    hits.sort_by(|left, right| {
        right
            .source_score
            .total_cmp(&left.source_score)
            .then_with(|| left.hit.evidence_id.cmp(&right.hit.evidence_id))
    });
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod candidate_tests;
