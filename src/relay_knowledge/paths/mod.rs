//! Platform path resolution for relay-knowledge runtime state.
//!
//! The module owns all default and override rules for config, data, state,
//! cache, log, temp, runtime, and service directories. It never reads the
//! process environment directly; callers pass the typed environment snapshot
//! produced by `env`.

use std::{
    error::Error,
    fmt,
    path::{Component, Path, PathBuf},
};

use crate::{
    env::{PathEnvOverrides, PlatformEnvironment, PlatformKind},
    project::{
        MODEL_CATALOG_CACHE_FILE_NAME, MODEL_FALLBACK_FILE_NAME, MODEL_PROFILES_FILE_NAME,
        VERSION_CHECK_CACHE_FILE_NAME,
    },
};

pub use crate::project::APP_DIR_NAME;

/// Resolved runtime directories used by CLI, Web, services, and future workers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub service_dir: PathBuf,
}

impl RuntimePaths {
    /// Resolves platform defaults and relay-specific overrides into absolute paths.
    pub fn resolve(
        environment: &PlatformEnvironment,
        overrides: &PathEnvOverrides,
    ) -> Result<Self, PathError> {
        let defaults = if let Some(root) = overrides.home.as_deref() {
            runtime_home_defaults(root)?
        } else {
            platform_defaults(environment)?
        };

        let resolved = Self {
            config_dir: override_path(
                PathPurpose::Config,
                defaults.config_dir,
                overrides.config_dir.as_deref(),
            )?,
            data_dir: override_path(
                PathPurpose::Data,
                defaults.data_dir,
                overrides.data_dir.as_deref(),
            )?,
            state_dir: override_path(
                PathPurpose::State,
                defaults.state_dir,
                overrides.state_dir.as_deref(),
            )?,
            cache_dir: override_path(
                PathPurpose::Cache,
                defaults.cache_dir,
                overrides.cache_dir.as_deref(),
            )?,
            log_dir: override_path(
                PathPurpose::Log,
                defaults.log_dir,
                overrides.log_dir.as_deref(),
            )?,
            temp_dir: override_path(
                PathPurpose::Temp,
                defaults.temp_dir,
                overrides.temp_dir.as_deref(),
            )?,
            runtime_dir: override_path(
                PathPurpose::Runtime,
                defaults.runtime_dir,
                overrides.runtime_dir.as_deref(),
            )?,
            service_dir: override_path(
                PathPurpose::Service,
                defaults.service_dir,
                overrides.service_dir.as_deref(),
            )?,
        };

        validate_all(&resolved)?;
        Ok(resolved)
    }

    /// Returns the JSONL audit log owned by resident agent protocol adapters.
    pub fn agent_audit_log_file(&self) -> PathBuf {
        self.log_dir.join("agent-audit.jsonl")
    }

    /// Returns the model provider profile configuration file.
    pub fn model_profiles_file(&self) -> PathBuf {
        self.config_dir.join(MODEL_PROFILES_FILE_NAME)
    }

    /// Returns the model provider fallback-policy configuration file.
    pub fn model_fallback_file(&self) -> PathBuf {
        self.config_dir.join(MODEL_FALLBACK_FILE_NAME)
    }

    /// Returns the cached public model catalog file.
    pub fn model_catalog_cache_file(&self) -> PathBuf {
        self.cache_dir.join(MODEL_CATALOG_CACHE_FILE_NAME)
    }

    /// Returns the cached version-check result.
    pub fn version_check_cache_file(&self) -> PathBuf {
        self.cache_dir.join(VERSION_CHECK_CACHE_FILE_NAME)
    }
}

/// Returns conservative user document roots for local file indexing.
pub fn default_user_document_roots(
    environment: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, PathError> {
    let home = match environment.platform {
        PlatformKind::Windows => environment
            .home_dir
            .as_deref()
            .map(|path| validate_path(PathPurpose::Home, path).map(|_| path.to_path_buf()))
            .transpose()?,
        _ => validated_optional(PathPurpose::Home, environment.home_dir.as_deref())?
            .map(Path::to_path_buf),
    };
    let Some(home) = home else {
        return Ok(Vec::new());
    };

    Ok(["Documents", "Desktop", "Downloads"]
        .into_iter()
        .map(|child| home.join(child))
        .collect())
}

/// Directory category attached to path validation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPurpose {
    Home,
    Config,
    Data,
    State,
    Cache,
    Log,
    Temp,
    Runtime,
    Service,
}

impl fmt::Display for PathPurpose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Home => write!(formatter, "home"),
            Self::Config => write!(formatter, "config"),
            Self::Data => write!(formatter, "data"),
            Self::State => write!(formatter, "state"),
            Self::Cache => write!(formatter, "cache"),
            Self::Log => write!(formatter, "log"),
            Self::Temp => write!(formatter, "temp"),
            Self::Runtime => write!(formatter, "runtime"),
            Self::Service => write!(formatter, "service"),
        }
    }
}

/// Path resolution or validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError {
    pub purpose: PathPurpose,
    pub kind: PathErrorKind,
}

impl PathError {
    fn missing_base(purpose: PathPurpose, variable: &'static str) -> Self {
        Self {
            purpose,
            kind: PathErrorKind::MissingBase { variable },
        }
    }

    fn relative(purpose: PathPurpose, path: &Path) -> Self {
        Self {
            purpose,
            kind: PathErrorKind::RelativePath {
                path: path.to_path_buf(),
            },
        }
    }

    fn parent_component(purpose: PathPurpose, path: &Path) -> Self {
        Self {
            purpose,
            kind: PathErrorKind::ParentComponent {
                path: path.to_path_buf(),
            },
        }
    }
}

/// Detailed path error category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathErrorKind {
    MissingBase { variable: &'static str },
    RelativePath { path: PathBuf },
    ParentComponent { path: PathBuf },
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            PathErrorKind::MissingBase { variable } => write!(
                formatter,
                "cannot resolve {} directory because {variable} is unavailable",
                self.purpose
            ),
            PathErrorKind::RelativePath { path } => write!(
                formatter,
                "{} directory must be absolute, got {}",
                self.purpose,
                path.display()
            ),
            PathErrorKind::ParentComponent { path } => write!(
                formatter,
                "{} directory must not contain '..', got {}",
                self.purpose,
                path.display()
            ),
        }
    }
}

impl Error for PathError {}

fn runtime_home_defaults(root: &Path) -> Result<RuntimePaths, PathError> {
    validate_path(PathPurpose::Home, root)?;

    Ok(RuntimePaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        cache_dir: root.join("cache"),
        log_dir: root.join("logs"),
        temp_dir: root.join("tmp"),
        runtime_dir: root.join("run"),
        service_dir: root.join("service"),
    })
}

fn platform_defaults(environment: &PlatformEnvironment) -> Result<RuntimePaths, PathError> {
    match environment.platform {
        PlatformKind::Macos => macos_defaults(environment),
        PlatformKind::Windows => windows_defaults(environment),
        PlatformKind::Unix | PlatformKind::Other => unix_defaults(environment),
    }
}

fn unix_defaults(environment: &PlatformEnvironment) -> Result<RuntimePaths, PathError> {
    let home = validated_optional(PathPurpose::Home, environment.home_dir.as_deref())?;
    let config_base = base_or_home_child(
        PathPurpose::Config,
        environment.xdg_config_home.as_deref(),
        home,
        ".config",
        PathBuf::from("/etc"),
    )?;
    let data_base = base_or_home_child(
        PathPurpose::Data,
        environment.xdg_data_home.as_deref(),
        home,
        ".local/share",
        PathBuf::from("/var/lib"),
    )?;
    let state_base = base_or_home_child(
        PathPurpose::State,
        environment.xdg_state_home.as_deref(),
        home,
        ".local/state",
        PathBuf::from("/var/lib"),
    )?;
    let cache_base = base_or_home_child(
        PathPurpose::Cache,
        environment.xdg_cache_home.as_deref(),
        home,
        ".cache",
        PathBuf::from("/var/cache"),
    )?;
    let temp_base = optional_or_default(
        PathPurpose::Temp,
        environment.temp_dir.as_deref(),
        PathBuf::from("/tmp"),
    )?;
    let state_dir = state_base.join(APP_DIR_NAME);
    let runtime_dir = if let Some(runtime_base) =
        validated_optional(PathPurpose::Runtime, environment.xdg_runtime_dir.as_deref())?
    {
        runtime_base.join(APP_DIR_NAME)
    } else {
        state_dir.join("run")
    };

    Ok(RuntimePaths {
        config_dir: config_base.join(APP_DIR_NAME),
        data_dir: data_base.join(APP_DIR_NAME),
        state_dir: state_dir.clone(),
        cache_dir: cache_base.join(APP_DIR_NAME),
        log_dir: state_dir.join("logs"),
        temp_dir: temp_base.join(APP_DIR_NAME),
        runtime_dir,
        service_dir: config_base.join(APP_DIR_NAME).join("service"),
    })
}

fn macos_defaults(environment: &PlatformEnvironment) -> Result<RuntimePaths, PathError> {
    let home = required_base(
        PathPurpose::Home,
        environment.home_dir.as_deref(),
        HOME_REQUIRED,
    )?;
    let application_support = home.join("Library").join("Application Support");
    let state_dir = application_support.join(APP_DIR_NAME).join("state");

    Ok(RuntimePaths {
        config_dir: application_support.join(APP_DIR_NAME).join("config"),
        data_dir: application_support.join(APP_DIR_NAME).join("data"),
        state_dir: state_dir.clone(),
        cache_dir: home.join("Library").join("Caches").join(APP_DIR_NAME),
        log_dir: home.join("Library").join("Logs").join(APP_DIR_NAME),
        temp_dir: optional_or_default(
            PathPurpose::Temp,
            environment.temp_dir.as_deref(),
            PathBuf::from("/tmp"),
        )?
        .join(APP_DIR_NAME),
        runtime_dir: state_dir.join("run"),
        service_dir: application_support.join(APP_DIR_NAME).join("service"),
    })
}

fn windows_defaults(environment: &PlatformEnvironment) -> Result<RuntimePaths, PathError> {
    let config_base = environment
        .app_data
        .as_deref()
        .map(|path| validate_path(PathPurpose::Config, path).map(|_| path.to_path_buf()))
        .transpose()?
        .or_else(|| {
            environment
                .home_dir
                .as_ref()
                .map(|home| home.join("AppData/Roaming"))
        })
        .ok_or_else(|| PathError::missing_base(PathPurpose::Config, "APPDATA or HOME"))?;
    let local_base = environment
        .local_app_data
        .as_deref()
        .map(|path| validate_path(PathPurpose::Data, path).map(|_| path.to_path_buf()))
        .transpose()?
        .or_else(|| {
            environment
                .home_dir
                .as_ref()
                .map(|home| home.join("AppData/Local"))
        })
        .ok_or_else(|| PathError::missing_base(PathPurpose::Data, "LOCALAPPDATA or HOME"))?;
    let root = local_base.join(APP_DIR_NAME);
    let temp_dir = match environment.temp_dir.as_deref() {
        Some(path) => {
            validate_path(PathPurpose::Temp, path)?;
            path.join(APP_DIR_NAME)
        }
        None => root.join("tmp"),
    };

    Ok(RuntimePaths {
        config_dir: config_base.join(APP_DIR_NAME),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        cache_dir: root.join("cache"),
        log_dir: root.join("logs"),
        temp_dir,
        runtime_dir: root.join("run"),
        service_dir: config_base.join(APP_DIR_NAME).join("service"),
    })
}

const HOME_REQUIRED: &str = "HOME";

fn base_or_home_child(
    purpose: PathPurpose,
    configured: Option<&Path>,
    home: Option<&Path>,
    home_child: &str,
    fallback_base: PathBuf,
) -> Result<PathBuf, PathError> {
    if let Some(path) = configured {
        validate_path(purpose, path)?;
        return Ok(path.to_path_buf());
    }

    if let Some(path) = home {
        validate_path(purpose, path)?;
        return Ok(path.join(home_child));
    }

    validate_path(purpose, &fallback_base)?;
    Ok(fallback_base)
}

fn required_base(
    purpose: PathPurpose,
    value: Option<&Path>,
    variable: &'static str,
) -> Result<PathBuf, PathError> {
    value
        .map(|path| validate_path(purpose, path).map(|_| path.to_path_buf()))
        .transpose()?
        .ok_or_else(|| PathError::missing_base(purpose, variable))
}

fn validated_optional(
    purpose: PathPurpose,
    value: Option<&Path>,
) -> Result<Option<&Path>, PathError> {
    if let Some(path) = value {
        validate_path(purpose, path)?;
    }

    Ok(value)
}

fn optional_or_default(
    purpose: PathPurpose,
    value: Option<&Path>,
    default: PathBuf,
) -> Result<PathBuf, PathError> {
    match value {
        Some(path) => {
            validate_path(purpose, path)?;
            Ok(path.to_path_buf())
        }
        None => {
            validate_path(purpose, &default)?;
            Ok(default)
        }
    }
}

fn override_path(
    purpose: PathPurpose,
    default: PathBuf,
    override_value: Option<&Path>,
) -> Result<PathBuf, PathError> {
    if let Some(path) = override_value {
        validate_path(purpose, path)?;
        Ok(path.to_path_buf())
    } else {
        Ok(default)
    }
}

fn validate_all(paths: &RuntimePaths) -> Result<(), PathError> {
    validate_path(PathPurpose::Config, &paths.config_dir)?;
    validate_path(PathPurpose::Data, &paths.data_dir)?;
    validate_path(PathPurpose::State, &paths.state_dir)?;
    validate_path(PathPurpose::Cache, &paths.cache_dir)?;
    validate_path(PathPurpose::Log, &paths.log_dir)?;
    validate_path(PathPurpose::Temp, &paths.temp_dir)?;
    validate_path(PathPurpose::Runtime, &paths.runtime_dir)?;
    validate_path(PathPurpose::Service, &paths.service_dir)
}

fn validate_path(purpose: PathPurpose, path: &Path) -> Result<(), PathError> {
    if !path.is_absolute() {
        return Err(PathError::relative(purpose, path));
    }

    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PathError::parent_component(purpose, path));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
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

        let paths = RuntimePaths::resolve(&unix_environment(), &overrides)
            .expect("overrides should resolve");

        assert_eq!(paths.config_dir, PathBuf::from("/custom/config"));
        assert_eq!(paths.data_dir, PathBuf::from("/custom/data"));
        assert_eq!(paths.state_dir, PathBuf::from("/custom/state"));
        assert_eq!(paths.cache_dir, PathBuf::from("/custom/cache"));
        assert_eq!(paths.log_dir, PathBuf::from("/custom/log"));
        assert_eq!(paths.temp_dir, PathBuf::from("/custom/tmp"));
        assert_eq!(paths.runtime_dir, PathBuf::from("/custom/run"));
        assert_eq!(paths.service_dir, PathBuf::from("/custom/service"));
    }
}
