//! Owns model fallback defaults and policy validation.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tokio::fs;

use super::{
    ModelProviderConfigService, ModelProviderError, persistence::write_json,
    profile_config::validate_profile_name,
};

/// Built-in fallback policy strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFallbackStrategy {
    SameProviderThenOtherProvider,
    OtherProviderOnly,
}

/// Model fallback policy used after retryable provider failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackPolicy {
    pub policy_id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub strategy: ModelFallbackStrategy,
    pub max_hops: u32,
    pub cooldown_seconds: u32,
}

/// Fallback config returned by Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackConfig {
    pub policies: Vec<ModelFallbackPolicy>,
}

fn default_fallback() -> ModelFallbackConfig {
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

fn validate_fallback_config(config: &ModelFallbackConfig) -> Result<(), ModelProviderError> {
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

impl ModelProviderConfigService {
    pub async fn fallback_config(&self) -> Result<ModelFallbackConfig, ModelProviderError> {
        match fs::read_to_string(self.paths.model_fallback_file()).await {
            Ok(raw) => serde_json::from_str(&raw).map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default_fallback()),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    pub async fn save_fallback_config(
        &self,
        config: ModelFallbackConfig,
    ) -> Result<ModelFallbackConfig, ModelProviderError> {
        validate_fallback_config(&config)?;
        write_json(self.paths.model_fallback_file(), &config).await?;
        Ok(config)
    }
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod tests;
