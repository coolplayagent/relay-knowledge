use crate::domain::FreshnessPolicy;

use super::HybridRetrievalRequest;

#[test]
fn new_retrieval_request_uses_bounded_human_defaults() {
    let request = HybridRetrievalRequest::new("lease recovery");

    assert_eq!(request.query, "lease recovery");
    assert_eq!(request.source_scope, None);
    assert_eq!(request.limit, 10);
    assert_eq!(request.freshness, FreshnessPolicy::default());
}
