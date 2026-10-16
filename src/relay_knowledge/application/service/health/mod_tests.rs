use super::*;
use crate::storage::SqliteStorageDiagnostics;

#[test]
fn maintenance_advice_uses_observed_pressure_and_age_without_claiming_timeout_cause() {
    let mut sqlite = SqliteStorageDiagnostics::default();
    assert!(storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, false).is_empty());
    let advice = storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, true);
    assert_eq!(advice.len(), 1);
    assert!(advice[0].contains("does not establish a WAL problem"));
    sqlite.wal_size_bytes = Some(LARGE_WAL_BYTES - 1);
    sqlite.last_maintenance_at_ms = Some(1);
    assert!(storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, false).is_empty());
    sqlite.wal_size_bytes = Some(LARGE_WAL_BYTES);
    let advice = storage_recommendations(&sqlite, MAINTENANCE_AGE_MS + 1, false);
    assert_eq!(advice.len(), 2);
    assert!(advice[0].contains("checkpoint"));
    assert!(advice[1].contains("service doctor"));
    sqlite.wal_size_bytes = Some(0);
    sqlite.last_maintenance_at_ms = Some(u64::MAX);
    assert!(storage_recommendations(&sqlite, 0, false).is_empty());
    sqlite.last_maintenance_error = Some("private failure details".into());
    let advice = storage_recommendations(&sqlite, 0, false);
    assert_eq!(advice.len(), 1);
    assert!(!advice[0].contains("private"));
}

#[tokio::test]
async fn degraded_health_never_reloads_model_profile_files() {
    use crate::{
        api::InterfaceKind,
        application::RuntimeConfiguration,
        env::{EnvironmentConfig, PlatformKind},
        storage::SqliteGraphStore,
    };
    let root = std::env::temp_dir().join(format!(
        "relay-health-profiles-{}-{}",
        std::process::id(),
        current_time_millis()
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    let profile = runtime.paths.model_profiles_file();
    let service = RelayKnowledgeService::with_store(
        runtime,
        Arc::new(SqliteGraphStore::open_in_memory().unwrap()),
    );
    let context = || RequestContext::for_interface(InterfaceKind::Cli);
    let baseline = service.health(context()).await.unwrap();
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(
        &profile,
        "invalid profile file that must not be read in fallback",
    )
    .unwrap();
    let cached = service
        .degraded_cached_health(context(), "storage_busy: timed out")
        .await;
    assert_eq!(
        cached.runtime.model_profiles,
        baseline.runtime.model_profiles
    );
    *service.health_cache.write().await = None;
    let cold = service
        .degraded_cached_health(context(), "storage_busy: timed out")
        .await;
    assert!(!cold.runtime.model_profiles.loaded);
    assert!(
        cold.runtime
            .model_profiles
            .error
            .unwrap()
            .contains("not inspected")
    );
    std::fs::remove_dir_all(root).unwrap();
}
