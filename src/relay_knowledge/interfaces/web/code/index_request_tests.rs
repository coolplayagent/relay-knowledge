use serde_json::json;

use super::*;
use crate::domain::{CodeRepositorySelector, CodeWorkspaceDetectionConfig};

fn payload() -> Value {
    json!({
        "alias": "relay",
        "ref": "main",
        "path_filters": [],
        "language_filters": []
    })
}

#[test]
fn defaults_workspace_detection_when_absent() {
    let request = code_index_request(&payload(), CodeIndexMode::Full).expect("request");
    assert_eq!(
        request.workspace_detection,
        CodeWorkspaceDetectionConfig::default()
    );
}

#[test]
fn parses_workspace_detection_config() {
    let mut payload = payload();
    payload["workspace_detection"] = json!({
        "enabled": true,
        "supported_formats": ["pnpm", "go_modules"]
    });

    let request = code_index_request(&payload, CodeIndexMode::Full).expect("request");

    assert!(request.workspace_detection.enabled);
    assert_eq!(
        request.workspace_detection.supported_formats,
        vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::GoModules,
        ]
    );
    assert_eq!(
        request.repository,
        CodeRepositorySelector::new("relay", "main", Vec::new(), Vec::new()).expect("selector")
    );
}

#[test]
fn maps_worktree_ref_to_overlay_request() {
    let mut payload = payload();
    payload["ref"] = json!("worktree");

    let request = code_index_request(&payload, CodeIndexMode::Full).expect("request");

    assert_eq!(request.mode, CodeIndexMode::WorktreeOverlay);
    assert_eq!(
        request.repository,
        CodeRepositorySelector::new("relay", "HEAD", Vec::new(), Vec::new()).expect("selector")
    );
}

#[test]
fn rejects_unsupported_workspace_detection_format() {
    let mut payload = payload();
    payload["workspace_detection"] = json!({
        "enabled": true,
        "supported_formats": ["unknown"]
    });

    let error = code_index_request(&payload, CodeIndexMode::Full).expect_err("error");
    assert!(
        error
            .message
            .contains("unsupported workspace_detection.supported_formats")
    );
}
