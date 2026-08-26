use super::*;

#[test]
fn request_bounds_query_and_limit() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
    assert!(
        BusinessKnowledgeQueryRequest::new(
            selector.clone(),
            None,
            Some("MRR".to_owned()),
            BusinessKnowledgeQueryKind::All,
            FreshnessPolicy::AllowStale,
            10,
        )
        .is_ok()
    );
    assert!(
        BusinessKnowledgeQueryRequest::new(
            selector,
            None,
            None,
            BusinessKnowledgeQueryKind::All,
            FreshnessPolicy::AllowStale,
            501,
        )
        .is_err()
    );
}
