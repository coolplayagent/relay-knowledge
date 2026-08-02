//! Unit contract for shared CLI and Web code-index mode semantics.

use super::*;
use crate::domain::FreshnessPolicy;

#[test]
fn mode_selection_reserves_the_exact_worktree_ref() {
    assert_eq!(
        mode_for_index_ref(WORKTREE_REF_SELECTOR),
        CodeIndexMode::WorktreeOverlay
    );
    assert_eq!(mode_for_index_ref("HEAD"), CodeIndexMode::Full);
    assert_eq!(mode_for_index_ref("Worktree"), CodeIndexMode::Full);
}

#[test]
fn worktree_selector_uses_head_and_preserves_scope_filters() {
    let selector = CodeRepositorySelector::new(
        "relay",
        WORKTREE_REF_SELECTOR,
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");

    let normalized = selector_for_index_request(selector.clone(), &CodeIndexMode::WorktreeOverlay);
    assert_eq!(normalized.repository, selector.repository);
    assert_eq!(normalized.ref_selector, WORKTREE_BASE_REF_SELECTOR);
    assert_eq!(normalized.path_filters, selector.path_filters);
    assert_eq!(normalized.language_filters, selector.language_filters);
    assert_eq!(
        selector_for_index_request(selector.clone(), &CodeIndexMode::Full),
        selector
    );
}

#[test]
fn request_normalization_changes_only_worktree_selectors() {
    let selector =
        CodeRepositorySelector::new("relay", WORKTREE_REF_SELECTOR, Vec::new(), Vec::new())
            .expect("selector should validate");
    let request = CodeIndexRequest {
        repository: selector,
        mode: CodeIndexMode::WorktreeOverlay,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    let normalized = normalize_index_request(request);
    assert_eq!(
        normalized.repository.ref_selector,
        WORKTREE_BASE_REF_SELECTOR
    );
    assert_eq!(normalized.mode, CodeIndexMode::WorktreeOverlay);
    assert_eq!(normalized.freshness_policy, FreshnessPolicy::AllowStale);
}
