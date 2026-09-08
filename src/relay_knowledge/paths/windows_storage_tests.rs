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
fn windows_profile_directory_identity_is_stable_and_account_scoped() {
    let alice = windows_data_directory(Path::new(r"C:\Users\Alice\AppData\Local"));
    assert_eq!(
        alice,
        PathBuf::from(
            "D:/relay-knowledge/users/dc1293f29700252a5b44de80167715a182d410224faa8aa434540635880df529/data"
        )
    );
    for spelling in [
        "c:/users/alice/appdata/local/",
        "C:/Users/./Alice/AppData/Local",
        r"C:\USERS\ALICE\APPDATA\LOCAL",
    ] {
        assert_eq!(windows_data_directory(Path::new(spelling)), alice);
    }
    for other in [
        r"C:\Users\Bob\AppData\Local",
        r"E:\Users\Alice\AppData\Local",
    ] {
        assert_ne!(windows_data_directory(Path::new(other)), alice);
    }
}

#[test]
fn windows_users_receive_distinct_main_databases_and_shards() {
    let fixture = Fixture::new();
    let first = windows_defaults(&fixture.environment().platform).unwrap();
    let mut other = fixture.environment();
    other.platform.local_app_data = Some(fixture.0.join("other-user/local"));
    let second = windows_defaults(&other.platform).unwrap();
    assert_ne!(first.database_file(), second.database_file());
    assert_ne!(
        first.repository_shard_database_file("repo:same"),
        second.repository_shard_database_file("repo:same")
    );
    assert!(first.data_dir.starts_with("D:/relay-knowledge/users"));
    assert!(second.data_dir.starts_with("D:/relay-knowledge/users"));
}

#[tokio::test]
async fn new_windows_install_selects_d_directory_without_creating_it() {
    let fixture = Fixture::new();
    let current = windows_defaults(&fixture.environment().platform)
        .unwrap()
        .data_dir;
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
    env.paths.data_dir = None;
    env.paths.home = Some(fixture.0.join("selected-home"));
    let selected = RuntimePaths::resolve_for_runtime(&env.platform, &env.paths)
        .await
        .unwrap();
    assert_eq!(selected.data_dir, fixture.0.join("selected-home/data"));
}

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
