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

#[test]
fn business_query_mode_is_derived_instead_of_trusting_wire_input() {
    let request = BusinessKnowledgeQueryRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        None,
        Some("MRR".into()),
        BusinessKnowledgeQueryKind::All,
        FreshnessPolicy::AllowStale,
        10,
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&request).unwrap()["mode"], "search");
    let mut json = serde_json::to_value(&request).unwrap();
    json["mode"] = serde_json::json!("untrusted-mode");
    let inbound: BusinessKnowledgeQueryRequest = serde_json::from_value(json).unwrap();
    assert_eq!(serde_json::to_value(&inbound).unwrap()["mode"], "search");
    assert_eq!(inbound.query.as_deref(), Some("MRR"));
    let mut list = inbound;
    list.query = None;
    list.domain = Some("sales".into());
    let json = serde_json::to_value(&list).unwrap();
    assert_eq!(json["mode"], "list");
    assert_eq!(json["domain"], "sales");
    assert!(json.get("query").is_none());
}
