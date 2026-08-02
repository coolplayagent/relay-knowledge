use super::*;

fn policy() -> AgentAccessPolicy {
    AgentAccessPolicy::new(vec!["docs".to_owned()], false, 10, 1024, 1000, false)
        .expect("policy should build")
}

#[test]
fn rejects_missing_scope_when_policy_requires_one() {
    let error = authorize_scope(None, &policy()).expect_err("missing scope should fail");

    assert_eq!(error.kind, AgentAdapterErrorKind::InvalidScope);
}

#[test]
fn authorizes_only_configured_scopes() {
    let allowed =
        authorize_scope(Some(" docs ".to_owned()), &policy()).expect("scope should be authorized");
    let denied =
        authorize_scope(Some("src".to_owned()), &policy()).expect_err("scope should be denied");

    assert_eq!(allowed.as_deref(), Some("docs"));
    assert_eq!(denied.kind, AgentAdapterErrorKind::PermissionDenied);
    assert!(denied.message.contains("src"));
    assert!(denied.message.contains("agent access policy"));
}

#[test]
fn rejects_limits_above_policy_budget() {
    let error = authorize_limit(Some(11), &policy()).expect_err("limit should fail");

    assert_eq!(error.kind, AgentAdapterErrorKind::LimitExceeded);
}
