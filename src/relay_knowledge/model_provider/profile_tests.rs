use super::*;
use crate::model_provider::profile_config::profile_response;
use crate::retrieval::ReadModelBackendConfig;

#[test]
fn redacts_secret_profile_fields() {
    let profile = StoredModelProfile::from_save_request(
        ModelProfileSaveRequest {
            provider: ModelProviderKind::OpenAiCompatible,
            model: "gpt-test".to_owned(),
            base_url: Some("https://example.test/v1".to_owned()),
            api_key: Some("secret".to_owned()),
            clear_api_key: false,
            headers: vec![ModelRequestHeader {
                name: "x-api-key".to_owned(),
                value: Some("hidden".to_owned()),
                secret: true,
                configured: false,
            }],
            ssl_verify: None,
            context_window: None,
            max_tokens: None,
            temperature: default_temperature(),
            top_p: default_top_p(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            capabilities: None,
            fallback_policy_id: None,
            fallback_priority: 0,
            catalog_provider_id: None,
            catalog_provider_name: None,
            catalog_model_name: None,
            is_default: true,
        },
        None,
    )
    .expect("profile should validate");

    let view = profile.to_view("default", true);

    assert!(view.api_key_configured);
    assert_eq!(view.headers[0].value, None);
    assert!(view.headers[0].configured);
}

#[test]
fn local_retrieval_does_not_create_runtime_profile() {
    let retrieval = ReadModelBackendConfig::local();
    let response = profile_response(None, &retrieval);

    assert!(response.profiles.is_empty());
    assert_eq!(response.default_profile, None);
}
