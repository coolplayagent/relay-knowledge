use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn fast_path_requires_bounded_exact_target_hits() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let callers_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector.clone(),
        CodeQueryKind::Callers,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callees_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callers_identity =
        call_identity_query(&callers_request).expect("callers identity should parse");
    let callees_identity =
        call_identity_query(&callees_request).expect("callees identity should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        11,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        true
    ));
    assert!(call_identity_hits_can_answer_without_fts(
        &callees_request,
        &callees_identity,
        3,
        false
    ));
    let broad_identity = call_identity_query(
        &CodeRetrievalRequest::new(
            "Table",
            CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
                .expect("selector should validate"),
            CodeQueryKind::Callees,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate"),
    )
    .expect("identity query should parse");
    assert!(!call_identity_hits_can_answer_without_fts(
        &callees_request,
        &broad_identity,
        1,
        false
    ));

    let narrowed_selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["src/table.cc".to_owned()],
        vec!["cpp".to_owned()],
    )
    .expect("selector should validate");
    let narrowed_request = CodeRetrievalRequest::new(
        "Run",
        narrowed_selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let narrowed_identity =
        call_identity_query(&narrowed_request).expect("identity query should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &narrowed_request,
        &narrowed_identity,
        2,
        false
    ));
}
