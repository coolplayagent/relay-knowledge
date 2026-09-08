use super::*;
use crate::env::EnvironmentConfig;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("relay-windows-data-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn environment(&self) -> EnvironmentConfig {
        EnvironmentConfig::from_pairs(
            PlatformKind::Windows,
            [
                ("APPDATA", self.0.join("roaming")),
                ("LOCALAPPDATA", self.0.join("local")),
                ("TEMP", self.0.join("tmp")),
            ],
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn windows_account_storage_survives_profile_relocation() {
    let sid = "S-1-5-21-1-2-3-1001";
    let data = windows_data_directory(sid).unwrap();
    assert_eq!(
        data,
        PathBuf::from("D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data")
    );
    let fixture = Fixture::new();
    let mut env = fixture.environment();
    let first = windows_defaults(&env.platform, Some(&data)).unwrap();
    env.platform.local_app_data = Some(fixture.0.join("relocated-profile/local"));
    env.platform.home_dir = Some(fixture.0.join("relocated-profile"));
    let moved =
        windows_defaults(&env.platform, Some(&windows_data_directory(sid).unwrap())).unwrap();
    assert_eq!(first.database_file(), moved.database_file());
    assert_eq!(first.repository_shards_dir(), moved.repository_shards_dir());
    let other = windows_defaults(
        &env.platform,
        Some(&windows_data_directory("S-1-5-21-1-2-3-1002").unwrap()),
    )
    .unwrap();
    assert_ne!(first.database_file(), other.database_file());
    assert_ne!(
        first.repository_shard_database_file("repo:same"),
        other.repository_shard_database_file("repo:same")
    );
}

#[test]
fn pinned_windows_sid_paths_recover_the_original_account_policy() {
    for path in [
        "D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data",
        r"d:\RELAY-KNOWLEDGE\Users\s-1-5-21-1-2-3-1001\DATA\",
        r"\\?\D:\relay-knowledge\users\S-1-5-21-1-2-3-1001\data",
        "D:/relay-knowledge/./users/S-1-5-21-1-2-3-1001/data/",
    ] {
        assert_eq!(
            windows_data_sid_from_path(Path::new(path))
                .unwrap()
                .as_deref(),
            Some("S-1-5-21-1-2-3-1001"),
            "{path}"
        );
    }
}

#[test]
fn only_the_reserved_windows_data_layout_restores_policy() {
    for path in [
        "E:/relay-knowledge/users/S-1-5-18/data",
        "D:/custom/users/S-1-5-18/data",
        "D:/relay-knowledge/profiles/S-1-5-18/data",
        "D:/relay-knowledge/users/S-1-5-18/config",
        "D:/relay-knowledge/users/S-1-5-18/data/custom",
        "D:/relay-knowledge/users",
        "C:/Users/example/AppData/Local/relay-knowledge/data",
    ] {
        assert_eq!(windows_data_sid_from_path(Path::new(path)).unwrap(), None);
    }
    assert!(
        windows_data_sid_from_path(Path::new("D:/relay-knowledge/users/S-1-5-invalid/data"))
            .is_err()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_service_data_override_preserves_the_installer_sid_without_provisioning() {
    let fixture = Fixture::new();
    let mut env = fixture.environment();
    // Simulate a service loading a path pinned by a different account; this
    // must not query the service token and replace the original directory SID.
    let sid = "S-1-5-21-1-2-3-123456";
    let data = windows_data_directory(sid).unwrap();
    let existed = data.exists();
    env.paths.data_dir = Some(data.clone());
    for use_home in [false, true] {
        if use_home {
            env.paths.data_dir = None;
            env.paths.home = Some(data.parent().unwrap().to_path_buf());
        }
        let selected = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
            .await
            .unwrap();
        assert_eq!(selected.windows_data_sid.as_deref(), Some(sid));
        assert_eq!(selected.data_dir, data);
        assert_eq!(data.exists(), existed);
    }
}

#[test]
fn lexical_windows_resolution_requires_an_explicit_data_directory() {
    let fixture = Fixture::new();
    let env = fixture.environment();
    let error = RuntimePaths::resolve(&env.platform, &env.paths).unwrap_err();
    assert!(error.to_string().contains("resolve_for_runtime"));
}

#[tokio::test]
async fn new_windows_install_selects_d_directory_without_creating_it() {
    let fixture = Fixture::new();
    let current = windows_data_directory("S-1-5-21-1-2-3-1001").unwrap();
    let legacy = fixture.0.join("absent-legacy");
    assert_eq!(
        select_windows_data_directory(&current, &legacy)
            .await
            .unwrap(),
        current
    );
    assert!(!legacy.exists());
}

#[tokio::test]
async fn windows_upgrade_keeps_legacy_database_recovery_files_and_shards_in_place() {
    let fixture = Fixture::new();
    let current = fixture.0.join("new-data");
    let legacy = fixture.0.join("legacy-data");
    fs::create_dir_all(legacy.join("stores/repositories/repo")).unwrap();
    for name in [
        "relay-knowledge.sqlite",
        "relay-knowledge.sqlite-wal",
        "relay-knowledge.sqlite-shm",
        "stores/repositories/repo/code.sqlite",
    ] {
        fs::write(legacy.join(name), b"preserved data").unwrap();
    }
    assert_eq!(
        select_windows_data_directory(&current, &legacy)
            .await
            .unwrap(),
        legacy
    );
    assert!(!current.exists());
    assert_eq!(
        fs::read(legacy.join("relay-knowledge.sqlite-wal")).unwrap(),
        b"preserved data"
    );
    assert_eq!(
        fs::read(legacy.join("stores/repositories/repo/code.sqlite")).unwrap(),
        b"preserved data"
    );
}

#[tokio::test]
async fn two_existing_windows_data_directories_require_explicit_selection() {
    let fixture = Fixture::new();
    let current = fixture.0.join("new");
    let legacy = fixture.0.join("legacy");
    fs::create_dir(&current).unwrap();
    fs::create_dir(&legacy).unwrap();
    let error = select_windows_data_directory(&current, &legacy)
        .await
        .unwrap_err();
    assert_eq!(
        error.kind,
        PathErrorKind::ConflictingDataDirectories {
            current,
            legacy: legacy.clone()
        }
    );
    assert!(error.to_string().contains(RELAY_KNOWLEDGE_DATA_DIR));
    assert_eq!(
        select_windows_data_directory(&legacy, &legacy)
            .await
            .unwrap(),
        legacy
    );
}

#[tokio::test]
async fn invalid_legacy_directory_does_not_silently_select_an_empty_store() {
    let fixture = Fixture::new();
    let legacy = fixture.0.join("file");
    fs::write(&legacy, b"not a directory").unwrap();
    let error = select_windows_data_directory(&fixture.0.join("new"), &legacy)
        .await
        .unwrap_err();
    assert!(matches!(
        error.kind,
        PathErrorKind::DataDirectoryProbe { .. }
    ));
    assert!(error.to_string().contains("not a directory"));
    assert!(!fixture.0.join("new").exists());
}

#[test]
fn inaccessible_or_timed_out_storage_probes_are_not_treated_as_missing() {
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::TimedOut,
        io::ErrorKind::NotADirectory,
    ] {
        let error = data_directory_probe_result(Path::new("/legacy"), Err(io::Error::from(kind)))
            .unwrap_err();
        assert!(matches!(
            error.kind,
            PathErrorKind::DataDirectoryProbe { .. }
        ));
        assert_eq!(error.purpose, PathPurpose::Data);
        assert!(error.to_string().contains(RELAY_KNOWLEDGE_DATA_DIR));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn dangling_legacy_directory_symlink_remains_selected() {
    let fixture = Fixture::new();
    let legacy = fixture.0.join("legacy-link");
    std::os::unix::fs::symlink(fixture.0.join("unmounted-data"), &legacy).unwrap();
    assert_eq!(
        select_windows_data_directory(&fixture.0.join("new"), &legacy)
            .await
            .unwrap(),
        legacy
    );
}

#[tokio::test]
async fn runtime_data_and_home_overrides_bypass_legacy_discovery() {
    let fixture = Fixture::new();
    let mut env = fixture.environment();
    let legacy = fixture.0.join("local/relay-knowledge/data");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, b"would fail discovery").unwrap();
    env.paths.data_dir = Some(fixture.0.join("selected-data"));
    let selected = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
        .await
        .unwrap();
    assert_eq!(selected.data_dir, fixture.0.join("selected-data"));
    assert_eq!(selected.windows_data_sid, None);
    env.paths.data_dir = None;
    env.paths.home = Some(fixture.0.join("selected-home"));
    let selected = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
        .await
        .unwrap();
    assert_eq!(selected.data_dir, fixture.0.join("selected-home/data"));
    assert_eq!(selected.windows_data_sid, None);
}

#[cfg(windows)]
#[tokio::test]
async fn windows_runtime_only_records_policy_without_provisioning_storage() {
    let fixture = Fixture::new();
    let env = fixture.environment();
    let sid = windows_storage::current_sid().await.unwrap();
    let current = windows_data_directory(&sid).unwrap();
    let existed = current.exists();
    let paths = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
        .await
        .unwrap();
    assert_eq!(paths.data_dir, current);
    assert_eq!(paths.windows_data_sid.as_deref(), Some(sid.as_str()));
    assert_eq!(
        current.exists(),
        existed,
        "configuration must not create the default data directory"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn runtime_resolution_discovers_legacy_data_without_overrides() {
    let fixture = Fixture::new();
    let env = fixture.environment();
    let legacy = fixture.0.join("local/relay-knowledge/data");
    fs::create_dir_all(&legacy).unwrap();
    let selected = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
        .await
        .unwrap();
    assert_eq!(selected.data_dir, legacy);
    assert_eq!(
        selected.config_dir,
        fixture.0.join("roaming/relay-knowledge")
    );
}
