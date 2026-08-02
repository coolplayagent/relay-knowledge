use super::reference_usage_context_bonus;
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
