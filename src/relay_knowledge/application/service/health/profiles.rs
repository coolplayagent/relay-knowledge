//! Single-flight profile reads retained across cancelled health requests.

use std::sync::{Arc, Mutex};

use crate::{
    model_provider::{ModelProfileRuntimeSummary, ModelProviderConfigService},
    retrieval::ReadModelBackendConfig,
};

#[derive(Clone)]
pub(in crate::application::service) struct HealthProfileReader {
    admission: Arc<tokio::sync::Semaphore>,
    cached: Arc<Mutex<Option<ModelProfileRuntimeSummary>>>,
}

impl Default for HealthProfileReader {
    fn default() -> Self {
        Self {
            admission: Arc::new(tokio::sync::Semaphore::new(1)),
            cached: Arc::new(Mutex::new(None)),
        }
    }
}

impl HealthProfileReader {
    /// Admits at most one read, including filesystem work outliving its caller.
    pub(in crate::application::service) async fn summary(
        &self,
        provider: ModelProviderConfigService,
        retrieval: ReadModelBackendConfig,
    ) -> ModelProfileRuntimeSummary {
        let Ok(permit) = Arc::clone(&self.admission).try_acquire_owned() else {
            let mut summary = self
                .cached
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
                .unwrap_or(ModelProfileRuntimeSummary {
                    loaded: false,
                    profile_count: 0,
                    default_profile: None,
                    error: None,
                });
            summary.error = Some("model-profile read is still in progress; cached values may be stale; inspect the configured filesystem before restarting the managed service".to_owned());
            return summary;
        };
        let cached = Arc::clone(&self.cached);
        // The worker owns admission until the filesystem read actually returns.
        // Dropping the request's JoinHandle cannot admit another detached read.
        let worker = tokio::spawn(async move {
            let _permit = permit;
            let summary = provider.profile_summary(&retrieval).await;
            *cached.lock().unwrap_or_else(|error| error.into_inner()) = Some(summary.clone());
            summary
        });
        worker.await.unwrap_or_else(|_| ModelProfileRuntimeSummary {
            loaded: false,
            profile_count: 0,
            default_profile: None,
            error: Some("model-profile diagnostic worker stopped before completing".to_owned()),
        })
    }
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
