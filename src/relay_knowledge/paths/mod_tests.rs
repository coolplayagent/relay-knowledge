use super::*;
use crate::env::{PathEnvOverrides, PlatformEnvironment, PlatformKind};

fn unix_environment() -> PlatformEnvironment {
    PlatformEnvironment {
        platform: PlatformKind::Unix,
        home_dir: Some(PathBuf::from("/home/alice")),
        xdg_config_home: Some(PathBuf::from("/config")),
        xdg_data_home: Some(PathBuf::from("/data")),
        xdg_state_home: Some(PathBuf::from("/state")),
        xdg_cache_home: Some(PathBuf::from("/cache")),
        xdg_runtime_dir: Some(PathBuf::from("/run/user/1000")),
        app_data: None,
        local_app_data: None,
        temp_dir: Some(PathBuf::from("/tmp")),
    }
}

#[test]
fn resolves_unix_platform_paths() {
    let paths = RuntimePaths::resolve(&unix_environment(), &PathEnvOverrides::default())
        .expect("paths should resolve");

    assert_eq!(paths.config_dir, PathBuf::from("/config/relay-knowledge"));
    assert_eq!(paths.data_dir, PathBuf::from("/data/relay-knowledge"));
    assert_eq!(paths.state_dir, PathBuf::from("/state/relay-knowledge"));
    assert_eq!(paths.cache_dir, PathBuf::from("/cache/relay-knowledge"));
    assert_eq!(paths.log_dir, PathBuf::from("/state/relay-knowledge/logs"));
    assert_eq!(
        paths.runtime_dir,
        PathBuf::from("/run/user/1000/relay-knowledge")
    );
}

#[test]
fn windows_tasklist_command_uses_existing_system_root() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-system-root-{}",
        std::process::id()
    ));
    let executable = root.join("System32").join("tasklist.exe");
    std::fs::create_dir_all(executable.parent().expect("tasklist parent"))
        .expect("system directory should be created");
    std::fs::write(&executable, b"test tasklist").expect("tasklist fixture should be written");
    assert_eq!(windows_tasklist_command(Some(root.as_os_str())), executable);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn windows_tasklist_command_falls_back_when_system_root_is_unavailable() {
    assert_eq!(
        windows_tasklist_command(Some(std::ffi::OsStr::new(
            "/missing/relay-knowledge-system-root"
        ))),
        PathBuf::from("tasklist.exe")
    );
}

#[test]
fn runtime_home_override_keeps_state_out_of_repository_paths() {
    let overrides = PathEnvOverrides {
        home: Some(PathBuf::from("/srv/relay")),
        ..PathEnvOverrides::default()
    };

    let paths =
        RuntimePaths::resolve(&unix_environment(), &overrides).expect("paths should resolve");

    assert_eq!(paths.config_dir, PathBuf::from("/srv/relay/config"));
    assert_eq!(paths.data_dir, PathBuf::from("/srv/relay/data"));
    assert_eq!(paths.cache_dir, PathBuf::from("/srv/relay/cache"));
    assert_eq!(paths.log_dir, PathBuf::from("/srv/relay/logs"));
}

#[test]
fn rejects_relative_overrides() {
    let overrides = PathEnvOverrides {
        data_dir: Some(PathBuf::from("relative-data")),
        ..PathEnvOverrides::default()
    };

    let error = RuntimePaths::resolve(&unix_environment(), &overrides)
        .expect_err("relative override should fail");

    assert_eq!(error.purpose, PathPurpose::Data);
    assert_eq!(
        error.kind,
        PathErrorKind::RelativePath {
            path: PathBuf::from("relative-data")
        }
    );
}

#[test]
fn rejects_parent_components() {
    let overrides = PathEnvOverrides {
        cache_dir: Some(PathBuf::from("/var/cache/../relay")),
        ..PathEnvOverrides::default()
    };

    let error = RuntimePaths::resolve(&unix_environment(), &overrides)
        .expect_err("parent component should fail");

    assert_eq!(error.purpose, PathPurpose::Cache);
    assert!(matches!(error.kind, PathErrorKind::ParentComponent { .. }));
}

#[test]
fn resolves_unix_service_paths_without_home() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Unix,
        home_dir: None,
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: None,
        local_app_data: None,
        temp_dir: None,
    };

    let paths = RuntimePaths::resolve(&environment, &PathEnvOverrides::default())
        .expect("service defaults should resolve without HOME");

    assert_eq!(paths.config_dir, PathBuf::from("/etc/relay-knowledge"));
    assert_eq!(paths.data_dir, PathBuf::from("/var/lib/relay-knowledge"));
    assert_eq!(paths.cache_dir, PathBuf::from("/var/cache/relay-knowledge"));
    assert_eq!(
        paths.runtime_dir,
        PathBuf::from("/var/lib/relay-knowledge/run")
    );
}

#[test]
fn windows_temp_dir_is_scoped_under_application_directory() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Windows,
        home_dir: None,
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: Some(PathBuf::from("/roaming")),
        local_app_data: Some(PathBuf::from("/local")),
        temp_dir: Some(PathBuf::from("/shared-temp")),
    };

    let paths = RuntimePaths::resolve(&environment, &PathEnvOverrides::default())
        .expect("windows paths should resolve");

    assert_eq!(
        paths.temp_dir,
        PathBuf::from("/shared-temp/relay-knowledge")
    );
}

#[test]
fn resolves_macos_application_support_paths() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Macos,
        home_dir: Some(PathBuf::from("/Users/alice")),
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: None,
        local_app_data: None,
        temp_dir: None,
    };

    let paths =
        RuntimePaths::resolve(&environment, &PathEnvOverrides::default()).expect("mac paths");

    assert_eq!(
        paths.config_dir,
        PathBuf::from("/Users/alice/Library/Application Support/relay-knowledge/config")
    );
    assert_eq!(
        paths.cache_dir,
        PathBuf::from("/Users/alice/Library/Caches/relay-knowledge")
    );
    assert_eq!(
        paths.service_dir,
        PathBuf::from("/Users/alice/Library/LaunchAgents")
    );
    assert_eq!(paths.temp_dir, PathBuf::from("/tmp/relay-knowledge"));
}

#[test]
fn macos_requires_home_directory() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Macos,
        home_dir: None,
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: None,
        local_app_data: None,
        temp_dir: None,
    };

    let error = RuntimePaths::resolve(&environment, &PathEnvOverrides::default())
        .expect_err("missing HOME should fail");

    assert_eq!(error.purpose, PathPurpose::Home);
    assert_eq!(
        error.to_string(),
        "cannot resolve home directory because HOME is unavailable"
    );
}

#[test]
fn windows_falls_back_to_home_appdata_paths() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Windows,
        home_dir: Some(PathBuf::from("/Users/Alice")),
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: None,
        local_app_data: None,
        temp_dir: None,
    };

    let paths = RuntimePaths::resolve(&environment, &PathEnvOverrides::default())
        .expect("windows fallback should resolve");

    assert_eq!(
        paths.config_dir,
        PathBuf::from("/Users/Alice/AppData/Roaming/relay-knowledge")
    );
    assert_eq!(
        paths.data_dir,
        PathBuf::from("/Users/Alice/AppData/Local/relay-knowledge/data")
    );
    assert_eq!(
        paths.temp_dir,
        PathBuf::from("/Users/Alice/AppData/Local/relay-knowledge/tmp")
    );
}

#[test]
fn per_directory_overrides_replace_defaults() {
    let overrides = PathEnvOverrides {
        config_dir: Some(PathBuf::from("/custom/config")),
        data_dir: Some(PathBuf::from("/custom/data")),
        state_dir: Some(PathBuf::from("/custom/state")),
        cache_dir: Some(PathBuf::from("/custom/cache")),
        log_dir: Some(PathBuf::from("/custom/log")),
        temp_dir: Some(PathBuf::from("/custom/tmp")),
        runtime_dir: Some(PathBuf::from("/custom/run")),
        service_dir: Some(PathBuf::from("/custom/service")),
        ..PathEnvOverrides::default()
    };

    let paths =
        RuntimePaths::resolve(&unix_environment(), &overrides).expect("overrides should resolve");

    assert_eq!(paths.config_dir, PathBuf::from("/custom/config"));
    assert_eq!(paths.data_dir, PathBuf::from("/custom/data"));
    assert_eq!(paths.state_dir, PathBuf::from("/custom/state"));
    assert_eq!(paths.cache_dir, PathBuf::from("/custom/cache"));
    assert_eq!(paths.log_dir, PathBuf::from("/custom/log"));
    assert_eq!(paths.temp_dir, PathBuf::from("/custom/tmp"));
    assert_eq!(paths.runtime_dir, PathBuf::from("/custom/run"));
    assert_eq!(paths.service_dir, PathBuf::from("/custom/service"));
}

#[test]
fn repository_shard_paths_are_safe_and_stable_under_data_dir() {
    let overrides = PathEnvOverrides {
        home: Some(std::env::temp_dir().join(format!(
            "relay-knowledge-shard-paths-{}",
            std::process::id()
        ))),
        ..PathEnvOverrides::default()
    };
    let paths =
        RuntimePaths::resolve(&unix_environment(), &overrides).expect("paths should resolve");

    let db_path = paths.repository_shard_database_file("git:/srv/repos/core");

    assert!(db_path.starts_with(&paths.data_dir));
    assert_eq!(
        db_path.file_name().and_then(|value| value.to_str()),
        Some(REPOSITORY_SHARD_DATABASE_FILE_NAME)
    );
    assert!(
        db_path
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("git__srv_repos_core-"))
    );
}
