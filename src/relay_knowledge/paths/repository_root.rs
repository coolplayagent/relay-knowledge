//! Repository-root discovery for repository-local contracts.
//!
//! This module owns path-based discovery for repository-scoped files such as
//! `.knowledge/knowledge-map.yaml`. Callers provide the starting directory;
//! process cwd lookup belongs to bootstrap.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::project::AGENT_CONTRACT_DIR_NAME;

/// Error raised before repository-root discovery can walk ancestors.
#[derive(Debug)]
pub enum RepositoryRootDiscoveryError {
    StartUnavailable { path: PathBuf, source: io::Error },
    StartNotDirectory { path: PathBuf },
    MarkerProbeFailed { path: PathBuf, source: io::Error },
}

impl fmt::Display for RepositoryRootDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartUnavailable { path, source } => {
                write!(
                    formatter,
                    "failed to inspect start directory '{}': {source}",
                    path.display()
                )
            }
            Self::StartNotDirectory { path } => {
                write!(
                    formatter,
                    "repository root search must start from a directory, got '{}'",
                    path.display()
                )
            }
            Self::MarkerProbeFailed { path, source } => {
                write!(
                    formatter,
                    "failed to inspect repository marker '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for RepositoryRootDiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StartUnavailable { source, .. } | Self::MarkerProbeFailed { source, .. } => {
                Some(source)
            }
            Self::StartNotDirectory { .. } => None,
        }
    }
}

/// Finds the repository root that owns repository-local knowledge contracts.
///
/// Discovery starts at `start` and walks ancestors. A `.git` directory/file or
/// `.knowledge` directory wins immediately. If neither exists, the nearest
/// `AGENTS.md` ancestor is used as a compatibility fallback.
pub fn discover_repository_root(
    start: &Path,
) -> Result<Option<PathBuf>, RepositoryRootDiscoveryError> {
    let metadata =
        fs::metadata(start).map_err(|source| RepositoryRootDiscoveryError::StartUnavailable {
            path: start.to_path_buf(),
            source,
        })?;
    if !metadata.is_dir() {
        return Err(RepositoryRootDiscoveryError::StartNotDirectory {
            path: start.to_path_buf(),
        });
    }

    let mut agents_root = None;
    for path in start.ancestors() {
        if marker_exists(path.join(".git"))? || marker_exists(path.join(AGENT_CONTRACT_DIR_NAME))? {
            return Ok(Some(path.to_path_buf()));
        }
        if agents_root.is_none() && marker_exists(path.join("AGENTS.md"))? {
            agents_root = Some(path.to_path_buf());
        }
    }

    Ok(agents_root)
}

fn marker_exists(path: PathBuf) -> Result<bool, RepositoryRootDiscoveryError> {
    path.try_exists()
        .map_err(|source| RepositoryRootDiscoveryError::MarkerProbeFailed { path, source })
}

#[cfg(test)]
#[path = "repository_root_tests.rs"]
mod tests;
