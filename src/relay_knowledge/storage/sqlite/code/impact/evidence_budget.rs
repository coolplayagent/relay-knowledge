//! Keeps ranked impact evidence bounded while considering every seed batch.

use crate::domain::CodeRetrievalHit;

use super::super::query::dedupe_sort_truncate;

pub(super) struct ImpactHits {
    hits: Vec<CodeRetrievalHit>,
    limit: usize,
}

impl ImpactHits {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            hits: Vec::with_capacity(limit.saturating_mul(2)),
            limit,
        }
    }

    pub(super) fn push(&mut self, hit: CodeRetrievalHit) {
        self.hits.push(hit);
        if self.hits.len() >= self.limit.saturating_mul(2).max(1) {
            dedupe_sort_truncate(&mut self.hits, self.limit);
        }
    }

    pub(super) fn finish(mut self) -> Vec<CodeRetrievalHit> {
        dedupe_sort_truncate(&mut self.hits, self.limit);
        self.hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_evidence_budget_suite_bounds_payloads_and_matches_exhaustive_ranking() {
        let seed: CodeRetrievalHit = serde_json::from_value(serde_json::json!({
            "repository_id":"repo", "scope_id":"scope", "resolved_commit_sha":"commit",
            "tree_hash":"tree", "path":"src/file.rs", "language_id":"rust",
            "byte_range":{"start":0,"end":1}, "line_range":{"start":1,"end":1},
            "retrieval_layers":["lexical"], "index_versions":[], "stale":false,
            "score":1.0, "excerpt":"evidence"
        }))
        .unwrap();
        for limit in [1, 7, 100] {
            let mut bounded = ImpactHits::new(limit);
            let mut exhaustive = Vec::new();
            for index in (0..2_001).rev() {
                let mut hit = seed.clone();
                hit.path = format!("src/{:04}.rs", index % 337);
                hit.score = (index % 5) as f64;
                exhaustive.push(hit.clone());
                bounded.push(hit);
                assert!(bounded.hits.len() < 2 * limit);
                assert_eq!(bounded.hits.capacity(), 2 * limit);
            }
            let mut later_provenance = seed.clone();
            later_provenance.path = "src/0000.rs".into();
            later_provenance.score = 0.0;
            later_provenance.retrieval_layers = vec![crate::domain::CodeRetrievalLayer::CallGraph];
            exhaustive.push(later_provenance.clone());
            bounded.push(later_provenance);
            assert!(bounded.hits.len() < 2 * limit);
            dedupe_sort_truncate(&mut exhaustive, limit);
            assert_eq!(bounded.finish(), exhaustive);
        }
    }
}
