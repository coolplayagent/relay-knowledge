use std::{fs, time::Duration};

use crate::{
    api::FileIndexRequest,
    application::RuntimeConfiguration,
    env::{EnvironmentConfig, PlatformKind},
    storage::StorageError,
};

use super::{
    test_support::{TempFixture, service_for_root},
    *,
};

#[test]
fn query_validation_helpers_reject_unbounded_inputs() {
    assert_eq!(required_query("  quarter  ".to_owned()).unwrap(), "quarter");
    assert!(required_query(" \t ".to_owned()).is_err());
    assert_eq!(bounded_limit(1).unwrap(), 1);
    assert!(bounded_limit(0).is_err());
    assert!(bounded_limit(MAX_FILE_QUERY_LIMIT + 1).is_err());
    assert_eq!(
        normalize_optional_text(Some(" root ".to_owned())).unwrap(),
        Some("root".to_owned())
    );
    assert!(normalize_optional_text(Some(" ".to_owned())).is_err());
    assert_eq!(normalize_optional_text(None).unwrap(), None);
}

#[test]
fn query_timeout_helpers_map_runtime_budget_and_storage_errors() {
    assert_eq!(query_timeout_ms(std::time::Duration::from_millis(125)), 125);
    assert!(storage_error_timed_out(&StorageError::InvalidInput(
        "file query timed out waiting for storage lock".to_owned()
    )));
    assert!(!storage_error_timed_out(&StorageError::InvalidInput(
        "different validation failure".to_owned()
    )));
}

#[tokio::test]
async fn explicit_roots_must_match_authorized_runtime_roots() {
    let fixture = TempFixture::new("authorized-roots");
    let service = service_for_root(fixture.path()).await;
    let authorized = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec![fixture.path().join(".").to_string_lossy().to_string()],
        })
        .expect("configured root spelling should be authorized");
    assert_eq!(authorized.len(), 1);

    let denied = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec![fixture.path().join("other").to_string_lossy().to_string()],
        })
        .expect_err("unconfigured root should be denied");
    assert!(denied.contains("is not configured"));

    let relative = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec!["relative/docs".to_owned()],
        })
        .expect_err("relative roots should be denied");
    assert!(relative.contains("absolute path"));
}

#[tokio::test]
async fn same_path_roots_remain_distinct_across_scopes() {
    let fixture = TempFixture::new("scope-roots");
    let home = fixture.path().join("home");
    let documents = home.join("Documents");
    fs::create_dir_all(&documents).expect("documents directory should be created");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", home.to_string_lossy().to_string()),
            ("TMPDIR", "/tmp".to_owned()),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                documents.to_string_lossy().to_string(),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS",
                "120000".to_owned(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    assert_eq!(runtime.file_index.scan_timeout, Duration::from_secs(120));

    let matching_roots = runtime
        .file_index
        .roots
        .iter()
        .filter(|root| root.root_path.as_path() == documents.as_path())
        .collect::<Vec<_>>();
    assert_eq!(matching_roots.len(), 2);
    assert_ne!(matching_roots[0].scope_id, matching_roots[1].scope_id);
}
