use super::*;

#[test]
fn rejects_invalid_sampling_boundaries() {
    assert!(validate_sampling(-0.1, 1.0, 30.0).is_err());
    assert!(validate_sampling(2.1, 1.0, 30.0).is_err());
    assert!(validate_sampling(0.7, 1.1, 30.0).is_err());
    assert!(validate_sampling(0.7, 1.0, 0.0).is_err());
    assert!(validate_sampling(0.7, 1.0, 300.1).is_err());
    assert!(validate_sampling(0.7, 1.0, 30.0).is_ok());
}

#[test]
fn normalizes_profile_names_and_provider_urls() {
    assert_eq!(
        validate_profile_name(" primary.profile ").unwrap(),
        "primary.profile"
    );
    assert!(validate_profile_name("bad profile").is_err());
    assert_eq!(
        normalized_base_url(
            ModelProviderKind::OpenAiCompatible,
            Some(" https://api.example.com/v1/ ".to_owned())
        )
        .unwrap(),
        "https://api.example.com/v1"
    );
    assert!(normalized_base_url(ModelProviderKind::OpenAiCompatible, None).is_err());
}

#[test]
fn rejects_duplicate_request_headers() {
    let headers = vec![
        ModelRequestHeader {
            name: "X-Api-Key".to_owned(),
            value: Some("one".to_owned()),
            secret: true,
            configured: false,
        },
        ModelRequestHeader {
            name: "x-api-key".to_owned(),
            value: Some("two".to_owned()),
            secret: true,
            configured: false,
        },
    ];

    assert!(validate_headers(headers, None).is_err());
}
