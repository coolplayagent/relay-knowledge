//! Owns persisted model profile workflows and runtime-profile resolution.

use std::collections::BTreeMap;

use tokio::fs;

use super::{
    ModelProfileRuntimeSummary, ModelProfileSaveRequest, ModelProfilesResponse,
    ModelProviderConfigService, ModelProviderError,
    persistence::write_json,
    profile::{DEFAULT_PROFILE_NAME, StoredModelProfile, StoredProfileFile},
    profile_config::profile_response,
    profile_config::runtime_profile_merge_base,
    profile_config::validate_profile_name,
};
use crate::retrieval::ReadModelBackendConfig;

impl ModelProviderConfigService {
    pub async fn profiles(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let file = self.load_profile_file().await?;
        Ok(profile_response(file, retrieval))
    }

    pub async fn profile_summary(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> ModelProfileRuntimeSummary {
        match self.profiles(retrieval).await {
            Ok(response) => ModelProfileRuntimeSummary {
                loaded: response.loaded,
                profile_count: response.profiles.len(),
                default_profile: response.default_profile,
                error: response.error,
            },
            Err(error) => ModelProfileRuntimeSummary {
                loaded: false,
                profile_count: 0,
                default_profile: None,
                error: Some(error.to_string()),
            },
        }
    }

    pub async fn save_profile(
        &self,
        name: &str,
        request: ModelProfileSaveRequest,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let name = validate_profile_name(name)?;
        let mut file = self
            .load_profile_file()
            .await?
            .unwrap_or_else(|| StoredProfileFile {
                default_profile: None,
                profiles: BTreeMap::new(),
            });
        let runtime_profile = runtime_profile_merge_base(&file, &name, retrieval);
        let existing = file.profiles.get(&name).or(runtime_profile.as_ref());
        let is_default = request.is_default || file.default_profile.is_none();
        let stored = StoredModelProfile::from_save_request(request, existing)?;
        file.profiles.insert(name.clone(), stored);
        if is_default {
            file.default_profile = Some(name);
            for (profile_name, profile) in &mut file.profiles {
                profile.is_default = file.default_profile.as_ref() == Some(profile_name);
            }
        }
        self.write_profile_file(&file).await?;
        Ok(profile_response(Some(file), retrieval))
    }

    pub async fn delete_profile(
        &self,
        name: &str,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let name = validate_profile_name(name)?;
        let mut file = self
            .load_profile_file()
            .await?
            .unwrap_or_else(|| StoredProfileFile {
                default_profile: None,
                profiles: BTreeMap::new(),
            });
        file.profiles.remove(&name);
        if file.default_profile.as_deref() == Some(&name) {
            file.default_profile = file.profiles.keys().next().cloned();
        }
        for (profile_name, profile) in &mut file.profiles {
            profile.is_default = file.default_profile.as_ref() == Some(profile_name);
        }
        self.write_profile_file(&file).await?;
        Ok(profile_response(Some(file), retrieval))
    }

    pub(super) async fn resolve_probe_profile(
        &self,
        retrieval: &ReadModelBackendConfig,
        profile_name: Option<String>,
        override_config: Option<ModelProfileSaveRequest>,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        match (profile_name, override_config) {
            (Some(name), Some(request)) => {
                let base = self.resolve_profile_by_name(retrieval, &name).await?;
                StoredModelProfile::from_save_request(request, Some(&base))
            }
            (Some(name), None) => self.resolve_profile_by_name(retrieval, &name).await,
            (None, Some(request)) => {
                let base = match self.resolve_default_profile(retrieval).await {
                    Ok(profile) => Some(profile),
                    Err(ModelProviderError::InvalidInput(message))
                        if message == "no model profile is configured" =>
                    {
                        None
                    }
                    Err(error) => return Err(error),
                };
                StoredModelProfile::from_save_request(request, base.as_ref())
            }
            (None, None) => self.resolve_default_profile(retrieval).await,
        }
    }

    async fn resolve_default_profile(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        let file = self.load_profile_file().await?;
        let response = profile_response(file.clone(), retrieval);
        let Some(default_name) = response.default_profile else {
            return Err(ModelProviderError::InvalidInput(
                "no model profile is configured".to_owned(),
            ));
        };
        self.resolve_profile_by_name(retrieval, &default_name).await
    }

    async fn resolve_profile_by_name(
        &self,
        retrieval: &ReadModelBackendConfig,
        name: &str,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        let name = validate_profile_name(name)?;
        if let Some(file) = self.load_profile_file().await? {
            if let Some(profile) = file.profiles.get(&name) {
                return Ok(profile.clone());
            }
        }
        if name == DEFAULT_PROFILE_NAME {
            if let Some(profile) = StoredModelProfile::from_runtime(retrieval) {
                return Ok(profile);
            }
        }
        Err(ModelProviderError::InvalidInput(format!(
            "model profile '{name}' was not found"
        )))
    }

    async fn load_profile_file(&self) -> Result<Option<StoredProfileFile>, ModelProviderError> {
        match fs::read_to_string(self.paths.model_profiles_file()).await {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    async fn write_profile_file(&self, file: &StoredProfileFile) -> Result<(), ModelProviderError> {
        write_json(self.paths.model_profiles_file(), file).await
    }
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
