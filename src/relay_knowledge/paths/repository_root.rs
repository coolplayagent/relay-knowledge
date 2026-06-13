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
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn root_search_walks_up_to_git_marker() {
        let root = temp_root("git-marker");
        let nested = root.join("src").join("module");
        fs::create_dir_all(root.join(".git")).expect("git marker should create");
        fs::create_dir_all(&nested).expect("nested dir should create");

        let discovered = discover_repository_root(&nested)
            .expect("search should succeed")
            .expect("root should be found");

        assert_eq!(discovered, root);
        let _ = fs::remove_dir_all(discovered);
    }

    #[test]
    fn root_search_walks_up_to_knowledge_contract_directory() {
        let root = temp_root("knowledge-marker");
        let nested = root.join("docs").join("architecture");
        fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
            .expect("knowledge marker should create");
        fs::create_dir_all(&nested).expect("nested dir should create");

        let discovered = discover_repository_root(&nested)
            .expect("search should succeed")
            .expect("root should be found");

        assert_eq!(discovered, root);
        let _ = fs::remove_dir_all(discovered);
    }

    #[test]
    fn root_search_falls_back_to_nearest_agents_file() {
        let root = temp_root("agents-marker");
        let nested = root.join("src").join("module");
        fs::create_dir_all(&nested).expect("nested dir should create");
        fs::write(
            root.join("AGENTS.md"),
            "Knowledge map: .knowledge/knowledge-map.yaml",
        )
        .expect("agents should write");

        let discovered = discover_repository_root(&nested)
            .expect("search should succeed")
            .expect("root should be found");

        assert_eq!(discovered, root);
        let _ = fs::remove_dir_all(discovered);
    }

    #[test]
    fn nested_agents_file_fallback_keeps_nearest_scope() {
        let root = temp_root("nested-agents-marker");
        let scoped = root.join("src");
        let nested = scoped.join("module");
        fs::create_dir_all(&nested).expect("nested dir should create");
        fs::write(root.join("AGENTS.md"), "Workspace instructions.").expect("root agents write");
        fs::write(scoped.join("AGENTS.md"), "Scoped instructions.").expect("scoped agents write");

        let discovered = discover_repository_root(&nested)
            .expect("search should succeed")
            .expect("root should be found");

        assert_eq!(discovered, scoped);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scoped_agents_file_does_not_override_git_root() {
        let root = temp_root("scoped-agents");
        let nested = root.join("src").join("module");
        fs::create_dir_all(root.join(".git")).expect("git marker should create");
        fs::create_dir_all(&nested).expect("nested dir should create");
        fs::write(nested.join("AGENTS.md"), "Scoped instructions.")
            .expect("scoped agents should write");

        let discovered = discover_repository_root(&nested)
            .expect("search should succeed")
            .expect("root should be found");

        assert_eq!(discovered, root);
        let _ = fs::remove_dir_all(discovered);
    }

    #[test]
    fn missing_markers_return_none() {
        let root = temp_root("missing-marker");
        let nested = root.join("src");
        fs::create_dir_all(&nested).expect("nested dir should create");

        let discovered = discover_repository_root(&nested).expect("search should succeed");

        assert_eq!(discovered, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_start_directory_returns_error() {
        let root = temp_root("missing-start");
        let missing = root.join("missing");
        fs::create_dir_all(&root).expect("root should create");

        let error = discover_repository_root(&missing).expect_err("missing start should fail");

        assert!(matches!(
            error,
            RepositoryRootDiscoveryError::StartUnavailable { .. }
        ));
        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "relay-knowledge-root-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ))
    }
}
