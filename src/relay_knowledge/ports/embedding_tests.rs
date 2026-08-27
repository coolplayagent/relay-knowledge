use super::*;

#[test]
fn provider_error_keeps_safe_diagnostics() {
    let error = EmbeddingProviderError {
        retry: ProviderRetryClass::Retryable,
        status_code: Some(429),
        code: "rate_limited".to_owned(),
        message: "provider capacity exhausted".to_owned(),
    };

    assert_eq!(
        error.to_string(),
        "rate_limited (429): provider capacity exhausted"
    );
}
