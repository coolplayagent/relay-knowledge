use super::*;

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
