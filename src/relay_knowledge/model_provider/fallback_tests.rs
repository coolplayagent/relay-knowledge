use super::*;
use crate::model_provider::test_support::test_service;

#[tokio::test]
async fn fallback_config_round_trips_and_rejects_invalid_values() {
    let service = test_service("fallback");
    let default = service
        .fallback_config()
        .await
        .expect("default fallback should load");
    assert_eq!(default.policies.len(), 2);

    let mut custom = default.clone();
    custom.policies[0].policy_id = "fast".to_owned();
    custom.policies[0].max_hops = 2;
    let saved = service
        .save_fallback_config(custom.clone())
        .await
        .expect("fallback should save");
    assert_eq!(saved, custom);
    assert_eq!(service.fallback_config().await.unwrap(), custom);

    custom.policies[0].cooldown_seconds = 3601;
    assert!(service.save_fallback_config(custom).await.is_err());
}

#[test]
fn rejects_duplicate_fallback_policies() {
    let config = ModelFallbackConfig {
        policies: vec![
            ModelFallbackPolicy {
                policy_id: "same".to_owned(),
                name: "Same".to_owned(),
                description: String::new(),
                enabled: true,
                strategy: ModelFallbackStrategy::OtherProviderOnly,
                max_hops: 1,
                cooldown_seconds: 1,
            },
            ModelFallbackPolicy {
                policy_id: "same".to_owned(),
                name: "Same".to_owned(),
                description: String::new(),
                enabled: true,
                strategy: ModelFallbackStrategy::OtherProviderOnly,
                max_hops: 1,
                cooldown_seconds: 1,
            },
        ],
    };

    assert!(validate_fallback_config(&config).is_err());
}
