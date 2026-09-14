use super::*;

#[test]
fn cursor_requires_supported_kind_policy_and_bounded_input() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", vec![], vec![]).unwrap();
    let request = SoftwareGlobalRequest::new(
        selector,
        SoftwareGlobalKind::Dependencies,
        FreshnessPolicy::AllowStale,
        1,
    )
    .unwrap();
    assert!(
        request
            .clone()
            .with_cursor(Some("sw1:token".into()))
            .is_ok()
    );
    for cursor in [String::new(), "x".repeat(4097)] {
        assert!(request.clone().with_cursor(Some(cursor)).is_err());
    }
    let mut unsupported = request.clone();
    unsupported.kind = SoftwareGlobalKind::Build;
    assert!(unsupported.with_cursor(Some("token".into())).is_err());
    let mut graph_only = request.clone();
    graph_only.freshness_policy = FreshnessPolicy::GraphOnly;
    assert!(graph_only.with_cursor(Some("token".into())).is_err());
    let mut invalid = request;
    invalid.limit = 0;
    assert!(invalid.validate().is_err());
    invalid.limit = 501;
    assert!(invalid.validate().is_err());
}

#[test]
fn global_kind_uses_stable_external_names() {
    assert_eq!(SoftwareGlobalKind::Dependencies.as_str(), "dependencies");
    assert_eq!(SoftwareGlobalKind::Build.as_str(), "build");
    assert_eq!(SoftwareGlobalKind::Statements.as_str(), "statements");
    assert_eq!(SoftwareGlobalKind::Conflicts.as_str(), "conflicts");
    assert_eq!(SoftwareGlobalKind::All.as_str(), "all");
}

#[test]
fn global_request_enforces_the_result_budget() {
    let repository = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let error = SoftwareGlobalRequest::new(
        repository,
        SoftwareGlobalKind::All,
        FreshnessPolicy::AllowStale,
        501,
    )
    .expect_err("oversized result budget should fail");

    assert_eq!(error.field, "limit");
}
