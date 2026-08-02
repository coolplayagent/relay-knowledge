use super::*;
use crate::model_provider::{
    ModelRequestHeader,
    test_support::{echo_request, openai_request, remote_retrieval, test_service},
};

#[tokio::test]
async fn profile_crud_preserves_secrets_and_redacts_responses() {
    let service = test_service("crud");
    let retrieval = ReadModelBackendConfig::local();
    let saved = service
        .save_profile(
            " primary ",
            openai_request("gpt-a", Some("secret-a")),
            &retrieval,
        )
        .await
        .expect("profile should save");

    assert_eq!(saved.default_profile.as_deref(), Some("primary"));
    assert_eq!(saved.profiles[0].base_url, "https://api.example.com/v1");
    assert!(saved.profiles[0].api_key_configured);
    assert_eq!(saved.profiles[0].headers[0].value, None);
    assert!(saved.profiles[0].headers[0].configured);

    let mut routine_update = openai_request("gpt-b", None);
    routine_update.ssl_verify = None;
    routine_update.capabilities = None;
    let updated = service
        .save_profile("primary", routine_update, &retrieval)
        .await
        .expect("profile should update");
    assert_eq!(updated.profiles[0].model, "gpt-b");
    assert!(updated.profiles[0].api_key_configured);
    assert_eq!(updated.profiles[0].ssl_verify, Some(true));
    assert_eq!(updated.profiles[0].capabilities.input.image, Some(true));

    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(raw.contains("secret-a"));
    assert!(raw.contains("header-secret"));
    assert!(
        !serde_json::to_string(&updated)
            .unwrap()
            .contains("secret-a")
    );

    let mut redacted_header_update = openai_request("gpt-c", None);
    redacted_header_update.headers = vec![ModelRequestHeader {
        name: "x-extra-secret".to_owned(),
        value: None,
        secret: true,
        configured: true,
    }];
    service
        .save_profile("primary", redacted_header_update, &retrieval)
        .await
        .expect("redacted header update should preserve stored header value");
    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(raw.contains("header-secret"));

    let mut clear_key_update = openai_request("gpt-d", None);
    clear_key_update.clear_api_key = true;
    clear_key_update.headers = vec![ModelRequestHeader {
        name: "x-extra-secret".to_owned(),
        value: Some("header-secret".to_owned()),
        secret: true,
        configured: false,
    }];
    let cleared = service
        .save_profile("primary", clear_key_update, &retrieval)
        .await
        .expect("header-auth update should clear stored api key");
    assert!(!cleared.profiles[0].api_key_configured);
    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(!raw.contains("secret-a"));
}

#[tokio::test]
async fn first_save_of_runtime_profile_preserves_environment_secret() {
    let service = test_service("runtime-secret");
    let retrieval = remote_retrieval();
    let profiles = service
        .profiles(&retrieval)
        .await
        .expect("runtime profile should load");
    assert_eq!(
        profiles.default_profile.as_deref(),
        Some(DEFAULT_PROFILE_NAME)
    );
    assert!(profiles.profiles[0].api_key_configured);
    assert_eq!(profiles.profiles[0].source, "environment");

    let mut request = openai_request("text-embedding-3-small", None);
    request.base_url = Some("https://api.openai.example/v1".to_owned());
    request.headers.clear();
    request.ssl_verify = None;
    request.capabilities = None;
    let saved = service
        .save_profile(DEFAULT_PROFILE_NAME, request, &retrieval)
        .await
        .expect("runtime profile edit should preserve secret");

    assert_eq!(saved.profiles[0].source, "config");
    assert!(saved.profiles[0].api_key_configured);
    assert_eq!(saved.profiles[0].capabilities.input.text, Some(true));
    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(raw.contains("env-secret"));
}

#[tokio::test]
async fn delete_profile_reassigns_default_and_reports_summary() {
    let service = test_service("delete");
    let retrieval = ReadModelBackendConfig::local();
    service
        .save_profile("first", echo_request("echo-a", true), &retrieval)
        .await
        .expect("first profile should save");
    service
        .save_profile("second", echo_request("echo-b", false), &retrieval)
        .await
        .expect("second profile should save");

    let response = service
        .delete_profile("first", &retrieval)
        .await
        .expect("profile should delete");
    assert_eq!(response.default_profile.as_deref(), Some("second"));
    assert_eq!(response.profiles.len(), 1);
    assert!(response.profiles[0].is_default);

    let summary = service.profile_summary(&retrieval).await;
    assert!(summary.loaded);
    assert_eq!(summary.profile_count, 1);
    assert_eq!(summary.default_profile.as_deref(), Some("second"));
}

#[tokio::test]
async fn profile_validation_rejects_invalid_inputs() {
    let service = test_service("validation");
    let retrieval = ReadModelBackendConfig::local();

    assert!(
        service
            .save_profile("bad name", echo_request("echo", true), &retrieval)
            .await
            .is_err()
    );
    let mut missing_auth = openai_request("gpt", None);
    missing_auth.headers.clear();
    assert!(
        service
            .save_profile("missing-auth", missing_auth, &retrieval)
            .await
            .is_err()
    );
    let mut bad_url = openai_request("gpt", Some("secret"));
    bad_url.base_url = Some("ftp://example.test".to_owned());
    assert!(
        service
            .save_profile("bad-url", bad_url, &retrieval)
            .await
            .is_err()
    );
    let mut bad_sampling = echo_request("echo", true);
    bad_sampling.temperature = 3.0;
    assert!(
        service
            .save_profile("bad-sampling", bad_sampling, &retrieval)
            .await
            .is_err()
    );
    let mut duplicate_headers = openai_request("gpt", None);
    duplicate_headers.headers = vec![
        ModelRequestHeader {
            name: "X-Key".to_owned(),
            value: Some("a".to_owned()),
            secret: true,
            configured: false,
        },
        ModelRequestHeader {
            name: "x-key".to_owned(),
            value: Some("b".to_owned()),
            secret: true,
            configured: false,
        },
    ];
    assert!(
        service
            .save_profile("dup-headers", duplicate_headers, &retrieval)
            .await
            .is_err()
    );
    let mut bad_header_name = openai_request("gpt", None);
    bad_header_name.headers = vec![ModelRequestHeader {
        name: "bad header".to_owned(),
        value: Some("secret".to_owned()),
        secret: true,
        configured: false,
    }];
    assert!(
        service
            .save_profile("bad-header-name", bad_header_name, &retrieval)
            .await
            .is_err()
    );
    let mut bad_header_value = openai_request("gpt", None);
    bad_header_value.headers = vec![ModelRequestHeader {
        name: "x-api-key".to_owned(),
        value: Some("line\nbreak".to_owned()),
        secret: true,
        configured: false,
    }];
    assert!(
        service
            .save_profile("bad-header-value", bad_header_value, &retrieval)
            .await
            .is_err()
    );
}
