use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn reference_identity_fast_path_requires_specific_bounded_hits() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "TargetThing",
        selector,
        CodeQueryKind::References,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let identity =
        SymbolIdentityQuery::from_query("TargetThing").expect("identity query should parse");

    assert!(reference_identity_hits_can_answer_without_fts(
        &request, &identity, 3, false
    ));
    assert!(!reference_identity_hits_can_answer_without_fts(
        &request, &identity, 11, false
    ));
    assert!(!reference_identity_hits_can_answer_without_fts(
        &request, &identity, 3, true
    ));
    let broad_identity =
        SymbolIdentityQuery::from_query("State").expect("identity query should parse");
    assert!(!reference_identity_hits_can_answer_without_fts(
        &request,
        &broad_identity,
        1,
        false
    ));
}
