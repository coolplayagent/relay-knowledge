// Direct tests for the version-check workflow.

use super::*;

#[test]
fn update_notice_requires_an_available_version_and_preserves_current_version() {
    let unavailable = VersionCheckResponse {
        project_name: PROJECT_NAME.to_owned(),
        current_version: "1.0.0".to_owned(),
        latest_version: None,
        update_available: false,
        source: None,
        release_url: None,
        checked_at_unix_ms: 42,
        diagnostics: Vec::new(),
    };
    assert!(notice_from_response(unavailable).is_none());

    let available = VersionCheckResponse {
        project_name: PROJECT_NAME.to_owned(),
        current_version: "1.0.0".to_owned(),
        latest_version: Some("1.1.0".to_owned()),
        update_available: true,
        source: Some("github".to_owned()),
        release_url: None,
        checked_at_unix_ms: 42,
        diagnostics: Vec::new(),
    };

    let notice = notice_from_response(available).expect("update should produce a notice");

    assert!(notice.contains("1.1.0 is available"));
    assert!(notice.contains("current 1.0.0"));
}
