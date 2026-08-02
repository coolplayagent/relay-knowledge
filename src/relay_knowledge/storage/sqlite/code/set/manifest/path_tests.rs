use super::{
    is_at_or_below_root, is_go_mod, join_workspace_path, workspace_pattern_matches,
    workspace_relative_path,
};

#[test]
fn workspace_relative_paths_require_a_segment_boundary() {
    assert_eq!(
        workspace_relative_path("packages/ui", "packages").as_deref(),
        Some("ui")
    );
    assert!(workspace_relative_path("packages-ui", "packages").is_none());
    assert!(is_at_or_below_root("packages/ui", "packages"));
    assert!(!is_at_or_below_root("packages-ui", "packages"));
}

#[test]
fn workspace_globs_match_bounded_segments() {
    assert!(workspace_pattern_matches(
        "packages/**/ui-*",
        "packages/web/ui-button"
    ));
    assert!(!workspace_pattern_matches(
        "packages/*",
        "packages/web/nested"
    ));
}

#[test]
fn workspace_joins_reject_parent_escapes_and_normalize_separators() {
    assert_eq!(
        join_workspace_path("examples", r#"demo\api"#).as_deref(),
        Some("examples/demo/api")
    );
    assert!(join_workspace_path("examples", "../outside").is_none());
    assert!(is_go_mod(r#"modules\api\go.mod"#));
}
