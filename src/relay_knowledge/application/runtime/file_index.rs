use std::{
    error::Error,
    fmt,
    path::{Component, PathBuf},
    time::Duration,
};

use crate::{
    env::{EnvironmentConfig, PlatformKind},
    paths::{PathError, default_user_document_roots},
};

/// Runtime budgets and authorized roots for local file-location indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRuntimeConfig {
    pub enabled: bool,
    pub roots: Vec<FileIndexRootConfig>,
    pub excludes: Vec<String>,
    pub max_depth: usize,
    pub max_file_bytes: u64,
    pub scan_interval: Duration,
    pub scan_timeout: Duration,
    pub max_files_per_root: usize,
    pub query_timeout: Duration,
}

impl FileIndexRuntimeConfig {
    pub const DEFAULT_MAX_DEPTH: usize = 32;
    pub const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
    pub const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_secs(900);
    pub const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(300);
    pub const DEFAULT_MAX_FILES_PER_ROOT: usize = 50_000;
    pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_millis(750);

    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, FileIndexRuntimeConfigError> {
        let mut roots = default_user_document_roots(&environment.platform)
            .map_err(FileIndexRuntimeConfigError::Paths)?
            .into_iter()
            .map(|path| FileIndexRootConfig::new("user-documents", path))
            .collect::<Vec<_>>();
        for root in split_semicolon(environment.file_index.roots.as_deref())? {
            roots.push(file_index_root_from_environment(
                "local-files",
                root,
                environment.platform.platform,
            )?);
        }
        roots.sort_by(|left, right| {
            left.scope_id
                .cmp(&right.scope_id)
                .then(left.root_id.cmp(&right.root_id))
        });
        roots.dedup_by(|left, right| {
            left.scope_id == right.scope_id && left.root_id == right.root_id
        });

        Ok(Self {
            enabled: environment.file_index.enabled.unwrap_or(false),
            roots,
            excludes: split_semicolon(environment.file_index.excludes.as_deref())?,
            max_depth: environment
                .file_index
                .max_depth
                .unwrap_or(Self::DEFAULT_MAX_DEPTH),
            max_file_bytes: environment
                .file_index
                .max_file_bytes
                .unwrap_or(Self::DEFAULT_MAX_FILE_BYTES),
            scan_interval: Duration::from_millis(
                environment
                    .file_index
                    .scan_interval_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_SCAN_INTERVAL)),
            ),
            scan_timeout: Duration::from_millis(
                environment
                    .file_index
                    .scan_timeout_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_SCAN_TIMEOUT)),
            ),
            max_files_per_root: environment
                .file_index
                .max_files_per_root
                .unwrap_or(Self::DEFAULT_MAX_FILES_PER_ROOT),
            query_timeout: Duration::from_millis(
                environment
                    .file_index
                    .query_timeout_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_QUERY_TIMEOUT)),
            ),
        })
    }
}

/// One authorized local file index root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRootConfig {
    pub scope_id: String,
    pub root_id: String,
    pub root_path: PathBuf,
}

impl FileIndexRootConfig {
    pub fn new(scope_id: impl Into<String>, root_path: PathBuf) -> Self {
        let root_path = normalize_file_index_root_path(root_path);
        let root_id = format!(
            "root-{:016x}",
            stable_hash64(root_path.to_string_lossy().as_bytes())
        );

        Self {
            scope_id: scope_id.into(),
            root_id,
            root_path,
        }
    }
}

fn normalize_file_index_root_path(root_path: PathBuf) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(&root_path) {
        return canonical;
    }

    let mut normalized = PathBuf::new();
    for component in root_path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => normalized.push(".."),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(value) => normalized.push(value),
        }
    }

    if normalized.as_os_str().is_empty() {
        root_path
    } else {
        normalized
    }
}

/// File index runtime validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIndexRuntimeConfigError {
    EmptyListValue,
    RelativeRoot(String),
    Paths(PathError),
}

impl fmt::Display for FileIndexRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyListValue => {
                write!(formatter, "file index lists must not contain empty values")
            }
            Self::RelativeRoot(path) => {
                write!(
                    formatter,
                    "file index root '{path}' must be an absolute path"
                )
            }
            Self::Paths(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for FileIndexRuntimeConfigError {}

fn split_semicolon(value: Option<&str>) -> Result<Vec<String>, FileIndexRuntimeConfigError> {
    value
        .map(|items| {
            items
                .split(';')
                .map(str::trim)
                .map(|item| {
                    if item.is_empty() {
                        Err(FileIndexRuntimeConfigError::EmptyListValue)
                    } else {
                        Ok(item.to_owned())
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn file_index_root_from_environment(
    scope_id: &'static str,
    root: String,
    platform: PlatformKind,
) -> Result<FileIndexRootConfig, FileIndexRuntimeConfigError> {
    if !is_absolute_file_index_root(&root, platform) {
        return Err(FileIndexRuntimeConfigError::RelativeRoot(root));
    }

    Ok(FileIndexRootConfig::new(scope_id, PathBuf::from(root)))
}

fn is_absolute_file_index_root(root: &str, platform: PlatformKind) -> bool {
    match platform {
        PlatformKind::Windows => is_absolute_windows_path(root),
        _ => PathBuf::from(root).is_absolute(),
    }
}

fn is_absolute_windows_path(root: &str) -> bool {
    let bytes = root.as_bytes();
    let drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if drive_rooted {
        return true;
    }

    if !(root.starts_with("\\\\") || root.starts_with("//")) {
        return false;
    }
    root[2..]
        .split(['\\', '/'])
        .filter(|component| !component.is_empty())
        .take(2)
        .count()
        == 2
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "file_index_tests.rs"]
mod tests;
