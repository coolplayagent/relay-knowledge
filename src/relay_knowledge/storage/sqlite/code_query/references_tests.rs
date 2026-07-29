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

#[test]
fn reference_usage_context_prioritizes_returns_and_function_type_annotations() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "InstanceContext",
        selector,
        CodeQueryKind::References,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let assignment = reference_usage_context_bonus(
        5.0,
        "value",
        "normalizeRoleId",
        Some("state.coordinatorRoleId = normalizeRoleId(roleId) || null;"),
        &request,
    );
    let returned = reference_usage_context_bonus(
        5.0,
        "value",
        "normalizeRoleId",
        Some("return normalizeRoleId(state.coordinatorRoleId);"),
        &request,
    );
    let type_signature = reference_usage_context_bonus(
        5.0,
        "type",
        "InstanceContext",
        Some("export function plan(input: Input, instance: InstanceContext) {"),
        &request,
    );
    let nested_type_signature = reference_usage_context_bonus(
        5.0,
        "type",
        "InstanceContext",
        Some("export function plan(input: Record<string, InstanceContext>) {"),
        &request,
    );

    assert!(returned > assignment);
    assert!(type_signature > 0.0);
    assert!(type_signature > nested_type_signature);
    assert!(
        reference_same_name_file_penalty(
            5.0,
            "packages/opencode/src/project/instance-context.ts",
            &request,
        ) < 0.0
    );
}

#[test]
fn plain_call_detection_requires_real_generic_call_shape() {
    assert!(identifier_is_plain_call("(value)"));
    assert!(identifier_is_plain_call("<Payload>(value)"));
    assert!(identifier_is_plain_call("<Map<Key, Value>>(value)"));
    assert!(!identifier_is_plain_call("< computeThreshold())"));
    assert!(!identifier_is_plain_call("< bar(baz)"));
    assert!(!identifier_is_plain_call("<Payload> + value"));
}
