use super::*;
use crate::{
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
};

#[tokio::test]
async fn occupied_reader_returns_explicitly_stale_cached_or_uninspected_summary() {
    let root = std::env::temp_dir().join("relay-health-profile-admission");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let provider = ModelProviderConfigService::new(paths);
    let reader = HealthProfileReader::default();
    let _occupied = reader.admission.clone().acquire_owned().await.unwrap();
    for cached in [
        None,
        Some(ModelProfileRuntimeSummary {
            loaded: true,
            profile_count: 7,
            default_profile: Some("cached".into()),
            error: None,
        }),
    ] {
        *reader.cached.lock().unwrap() = cached.clone();
        let summary = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            reader
                .clone()
                .summary(provider.clone(), ReadModelBackendConfig::local()),
        )
        .await
        .unwrap();
        assert_eq!(summary.loaded, cached.is_some());
        assert_eq!(
            summary.profile_count,
            cached.map_or(0, |value| value.profile_count)
        );
        assert!(
            summary
                .error
                .unwrap()
                .contains("cached values may be stale")
        );
    }
}
