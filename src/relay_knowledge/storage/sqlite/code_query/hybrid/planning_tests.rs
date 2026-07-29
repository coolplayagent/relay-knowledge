use super::*;
use crate::domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy};

#[test]
fn conversion_expansion_intent_accepts_scored_conversion_verbs() {
    for verb in ["adapt", "map", "normalize"] {
        assert!(
            hybrid_query_has_conversion_expansion_intent(&format!(
                "provider response parts {verb} shared event"
            )),
            "{verb} should request conversion expansion"
        );
    }
}

#[test]
fn conversion_expansion_intent_accepts_cased_conversion_verbs() {
    for query in ["Convert Common Chunk", "Map Provider Response Parts"] {
        assert!(
            hybrid_query_has_conversion_expansion_intent(query),
            "{query} should request conversion expansion"
        );
    }
}

#[test]
fn strict_hybrid_chunk_candidate_limit_stays_bounded() {
    assert_eq!(
        strict_hybrid_chunk_candidate_limit(&hybrid_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            10,
        )),
        60
    );
    assert_eq!(
        strict_hybrid_chunk_candidate_limit(&hybrid_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            40,
        )),
        120
    );
}

fn hybrid_request(query: &str, limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}
