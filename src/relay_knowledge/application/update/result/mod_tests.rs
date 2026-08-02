// Direct tests for version-check response contracts.

use super::*;

#[test]
fn version_check_result_round_trips_diagnostics_without_losing_source_identity() {
    let response = VersionCheckResponse {
        project_name: "relay-knowledge".to_owned(),
        current_version: "1.0.0".to_owned(),
        latest_version: Some("1.1.0".to_owned()),
        update_available: true,
        source: Some("github".to_owned()),
        release_url: Some("https://example.invalid/release".to_owned()),
        checked_at_unix_ms: 42,
        diagnostics: vec![VersionCheckDiagnostic {
            source: Some("crates.io".to_owned()),
            code: "http_status".to_owned(),
            message: "temporary failure".to_owned(),
            retryable: true,
        }],
    };

    let bytes = serde_json::to_vec(&response).expect("response should serialize");
    let decoded =
        serde_json::from_slice::<VersionCheckResponse>(&bytes).expect("response should parse");

    assert_eq!(decoded, response);
}
