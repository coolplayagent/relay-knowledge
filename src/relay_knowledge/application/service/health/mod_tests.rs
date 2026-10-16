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
    sqlite.last_maintenance_error = None;
    sqlite.last_maintenance_at_ms = None;
    sqlite.journal_mode = "wal".into();
    assert!(storage_recommendations(&sqlite, 0, false)[0].contains("history is missing"));
    sqlite.journal_mode = "memory".into();
    assert!(storage_recommendations(&sqlite, 0, false).is_empty());
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_storage_snapshot_cannot_wait_unboundedly_for_model_profiles() {
    use crate::{
        api::InterfaceKind,
        application::RuntimeConfiguration,
        env::{EnvironmentConfig, PlatformKind},
        storage::SqliteGraphStore,
    };
    use std::{
        io::ErrorKind,
        os::unix::fs::OpenOptionsExt,
        time::{Duration, Instant},
    };
    let root = std::env::temp_dir().join(format!(
        "relay-health-profile-deadline-{}-{}",
        std::process::id(),
        current_time_millis()
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [
            ("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap()),
            ("RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS", "500"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "1000"),
        ],
    )
    .unwrap();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    let profile = runtime.paths.model_profiles_file();
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&profile)
            .status()
            .unwrap()
            .success()
    );
    let (opened, reader_started) = tokio::sync::oneshot::channel();
    let (release, wait_release) = std::sync::mpsc::channel::<()>();
    // A test-only FIFO models an unavailable profile filesystem. The writer
    // confirms profile loading began after storage succeeded, then withholds EOF.
    let writer_profile = profile.clone();
    let writer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let file = loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&writer_profile)
            {
                Ok(file) => break Some(file),
                Err(error)
                    if error.raw_os_error() == Some(libc::ENXIO)
                        || error.kind() == ErrorKind::Interrupted =>
                {
                    if Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("profile FIFO: {error}"),
            }
        };
        if file.is_some() {
            let _ = opened.send(());
            let _ = wait_release.recv_timeout(Duration::from_secs(3));
        }
        drop(file);
    });
    let service = RelayKnowledgeService::with_store(
        runtime,
        Arc::new(SqliteGraphStore::open_in_memory().unwrap()),
    );
    let first_probe = service.clone();
    let request = tokio::spawn(async move {
        first_probe
            .health(RequestContext::for_interface(InterfaceKind::Web))
            .await
    });
    let opened = tokio::time::timeout(Duration::from_secs(2), reader_started).await;
    let response = tokio::time::timeout(Duration::from_millis(900), request).await;
    let mut repeated = Vec::new();
    for _ in 0..4 {
        repeated.push(
            tokio::time::timeout(
                Duration::from_millis(200),
                service
                    .clone()
                    .health(RequestContext::for_interface(InterfaceKind::Web)),
            )
            .await,
        );
    }
    let _ = release.send(());
    writer.join().unwrap();
    for probe in repeated {
        let health = probe.unwrap().unwrap();
        assert!(
            health
                .runtime
                .model_profiles
                .error
                .unwrap()
                .contains("still in progress")
        );
    }
    assert!(
        opened.unwrap().is_ok(),
        "successful storage read must reach profile loading"
    );
    let health = response.unwrap().unwrap().unwrap();
    assert!(!health.healthy);
    assert!(health.degraded_reason.unwrap().starts_with("storage_busy:"));
    std::fs::remove_file(&profile).unwrap();
    std::fs::write(&profile, r#"{"default_profile":null,"profiles":{}}"#).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let health = service
                .health(RequestContext::for_interface(InterfaceKind::Web))
                .await
                .unwrap();
            if health.runtime.model_profiles.loaded && health.runtime.model_profiles.error.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the reader must recover after the filesystem read completes");
    std::fs::remove_dir_all(root).unwrap();
}
