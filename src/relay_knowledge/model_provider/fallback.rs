//! Owns model fallback defaults and policy validation.

use std::collections::BTreeSet;

use super::profile_config::validate_profile_name;
use super::*;

pub(super) fn default_fallback() -> ModelFallbackConfig {
    ModelFallbackConfig {
        policies: vec![
            ModelFallbackPolicy {
                policy_id: "same_provider_then_other_provider".to_owned(),
                name: "Same Provider Then Other Provider".to_owned(),
                description: "Retry same-provider alternatives before switching providers."
                    .to_owned(),
                enabled: true,
                strategy: ModelFallbackStrategy::SameProviderThenOtherProvider,
                max_hops: 3,
                cooldown_seconds: 60,
            },
            ModelFallbackPolicy {
                policy_id: "other_provider_only".to_owned(),
                name: "Other Provider Only".to_owned(),
                description: "Fail over directly to profiles from other providers.".to_owned(),
                enabled: true,
                strategy: ModelFallbackStrategy::OtherProviderOnly,
                max_hops: 3,
                cooldown_seconds: 60,
            },
        ],
    }
}

pub(super) fn validate_fallback_config(
    config: &ModelFallbackConfig,
) -> Result<(), ModelProviderError> {
    let mut ids = BTreeSet::new();
    for policy in &config.policies {
        let id = validate_profile_name(&policy.policy_id)?;
        if !ids.insert(id.clone()) {
            return Err(ModelProviderError::InvalidInput(format!(
                "duplicate fallback policy id '{id}'"
            )));
        }
        if policy.max_hops == 0 || policy.cooldown_seconds > 3600 {
            return Err(ModelProviderError::InvalidInput(
                "fallback policy max_hops must be positive and cooldown_seconds <= 3600".to_owned(),
            ));
        }
    }
    Ok(())
}
