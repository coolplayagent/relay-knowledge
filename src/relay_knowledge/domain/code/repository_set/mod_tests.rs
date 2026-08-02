//! Direct contracts for repository-set requests and lifecycle state.

use super::*;

#[test]
fn repository_set_requests_normalize_inputs_and_defaults() {
    let create = CodeRepositorySetCreateRequest::new(
        " workspace ",
        Some(" grouped repos ".to_owned()),
        None,
    )
    .expect("create request should validate");
    assert_eq!(create.alias, "workspace");
    assert_eq!(create.description.as_deref(), Some("grouped repos"));
    assert_eq!(create.default_ref_policy_json, "{\"default_ref\":\"HEAD\"}");

    let add = CodeRepositorySetAddMemberRequest::new(
        "workspace",
        "core",
        "HEAD",
        vec![" src ".to_owned(), "src".to_owned()],
        vec![" rust ".to_owned(), "rust".to_owned()],
        10,
    )
    .expect("member request should validate");
    assert_eq!(add.path_filters, ["src"]);
    assert_eq!(add.language_filters, ["rust"]);

    let remove = CodeRepositorySetRemoveMemberRequest::new(" workspace ", " core ")
        .expect("remove request should validate");
    assert_eq!(remove.set_alias, "workspace");
    assert_eq!(remove.repository_alias, "core");

    let query = CodeRepositorySetQueryRequest::new(
        "workspace",
        "RetryPolicy",
        CodeQueryKind::Definition,
        50,
        FreshnessPolicy::WaitUntilFresh,
        vec!["src".to_owned(), "src".to_owned()],
        Vec::new(),
    )
    .expect("query request should validate");
    assert_eq!(query.limit, 50);
    assert_eq!(query.path_filters, ["src"]);
    assert_eq!(query.freshness_policy, FreshnessPolicy::WaitUntilFresh);
}

#[test]
fn repository_set_requests_reject_invalid_boundaries() {
    assert!(
        CodeRepositorySetCreateRequest::new(" ", None, None)
            .expect_err("blank alias should fail")
            .to_string()
            .contains("set_alias")
    );
    assert!(
        CodeRepositorySetAddMemberRequest::new("workspace", "core", " ", Vec::new(), Vec::new(), 0)
            .expect_err("blank ref should fail")
            .to_string()
            .contains("ref_selector")
    );
    assert!(
        CodeRepositorySetQueryRequest::new(
            "workspace",
            "query",
            CodeQueryKind::Hybrid,
            0,
            FreshnessPolicy::AllowStale,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("zero limit should fail")
        .to_string()
        .contains("greater than zero")
    );
    assert!(
        CodeRepositorySetQueryRequest::new(
            "workspace",
            "query",
            CodeQueryKind::Hybrid,
            51,
            FreshnessPolicy::AllowStale,
            Vec::new(),
            Vec::new(),
        )
        .expect_err("oversized limit should fail")
        .to_string()
        .contains("50 or less")
    );
    assert!(
        CodeRepositorySetRemoveMemberRequest::new("workspace", " ")
            .expect_err("blank repository alias should fail")
            .to_string()
            .contains("repository_alias")
    );
}

#[test]
fn repository_set_refresh_task_states_have_stable_wire_values() {
    for (state, wire, unfinished) in [
        (CodeRepositorySetRefreshTaskState::Queued, "queued", true),
        (CodeRepositorySetRefreshTaskState::Running, "running", true),
        (
            CodeRepositorySetRefreshTaskState::Succeeded,
            "succeeded",
            false,
        ),
        (
            CodeRepositorySetRefreshTaskState::Retrying,
            "retrying",
            true,
        ),
        (
            CodeRepositorySetRefreshTaskState::DeadLetter,
            "dead_letter",
            false,
        ),
    ] {
        assert_eq!(state.as_str(), wire);
        assert_eq!(
            CodeRepositorySetRefreshTaskState::parse(wire).expect("wire state should parse"),
            state
        );
        assert_eq!(state.is_unfinished(), unfinished);
    }
    assert!(
        CodeRepositorySetRefreshTaskState::parse("mystery")
            .expect_err("unknown state should fail")
            .to_string()
            .contains("unknown repository set refresh task state")
    );
}
