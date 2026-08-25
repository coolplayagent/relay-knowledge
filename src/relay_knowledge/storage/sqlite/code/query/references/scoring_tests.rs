use super::{
    REFERENCE_REPEATED_GROUP_MAX_BONUS, reference_usage_context_bonus,
    repeated_reference_group_bonus,
};
use crate::domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy};

#[test]
fn reference_usage_context_prioritizes_returns_over_assignments() {
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
    assert!(returned > assignment);
}

#[test]
fn repeated_reference_group_bonus_is_bounded_and_requires_query_evidence() {
    assert_eq!(repeated_reference_group_bonus(5.0, 1), 0.0);
    assert_eq!(repeated_reference_group_bonus(0.0, 8), 0.0);
    assert!(repeated_reference_group_bonus(5.0, 4) > repeated_reference_group_bonus(5.0, 2));
    assert_eq!(
        repeated_reference_group_bonus(5.0, usize::MAX),
        REFERENCE_REPEATED_GROUP_MAX_BONUS
    );
}
