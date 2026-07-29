//! Platform-specific environment inputs and normalization.

use std::{env as process_env, ffi::OsString};

use super::{
    EnvError,
    value_parser::{EnvironmentValues, first_path_var, path_var},
};

pub(super) const HOME: &str = "HOME";
const WINDOWS_SYSTEM_ROOT: &str = "SystemRoot";
const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const XDG_DATA_HOME: &str = "XDG_DATA_HOME";
const XDG_STATE_HOME: &str = "XDG_STATE_HOME";
const XDG_CACHE_HOME: &str = "XDG_CACHE_HOME";
const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const APPDATA: &str = "APPDATA";
const LOCALAPPDATA: &str = "LOCALAPPDATA";
pub(super) const TMPDIR: &str = "TMPDIR";
pub(super) const TEMP: &str = "TEMP";
pub(super) const TMP: &str = "TMP";

pub(crate) fn windows_system_root_from_process() -> Option<OsString> {
    process_env::var_os(WINDOWS_SYSTEM_ROOT).filter(|value| !value.is_empty())
}

/// Operating-system family used by path resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    Unix,
    Macos,
    Windows,
    Other,
}

impl PlatformKind {
    /// Detects the current target platform without consulting environment state.
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(unix) {
            Self::Unix
        } else {
            Self::Other
        }
    }
}

/// Platform directory inputs captured from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformEnvironment {
    pub platform: PlatformKind,
    pub home_dir: Option<std::path::PathBuf>,
    pub xdg_config_home: Option<std::path::PathBuf>,
    pub xdg_data_home: Option<std::path::PathBuf>,
    pub xdg_state_home: Option<std::path::PathBuf>,
    pub xdg_cache_home: Option<std::path::PathBuf>,
    pub xdg_runtime_dir: Option<std::path::PathBuf>,
    pub app_data: Option<std::path::PathBuf>,
    pub local_app_data: Option<std::path::PathBuf>,
    pub temp_dir: Option<std::path::PathBuf>,
}

pub(super) fn platform_environment(
    values: &EnvironmentValues,
    platform: PlatformKind,
) -> Result<PlatformEnvironment, EnvError> {
    let temp_variables: &[&'static str] = if platform == PlatformKind::Windows {
        &[TEMP, TMP, TMPDIR]
    } else {
        &[TMPDIR, TEMP, TMP]
    };

    Ok(PlatformEnvironment {
        platform,
        home_dir: path_var(values, HOME)?,
        xdg_config_home: path_var(values, XDG_CONFIG_HOME)?,
        xdg_data_home: path_var(values, XDG_DATA_HOME)?,
        xdg_state_home: path_var(values, XDG_STATE_HOME)?,
        xdg_cache_home: path_var(values, XDG_CACHE_HOME)?,
        xdg_runtime_dir: path_var(values, XDG_RUNTIME_DIR)?,
        app_data: path_var(values, APPDATA)?,
        local_app_data: path_var(values, LOCALAPPDATA)?,
        temp_dir: first_path_var(values, temp_variables)?,
    })
}

pub(super) fn normalize_key(platform: PlatformKind, key: OsString) -> OsString {
    if platform == PlatformKind::Windows {
        key.to_str()
            .map(|value| OsString::from(value.to_ascii_uppercase()))
            .unwrap_or(key)
    } else {
        key
    }
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod platform_tests;
