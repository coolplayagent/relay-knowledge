use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::evaluator::runtime::{
    contracts::EvalRuntime,
    workdir::{validate_direct_child, validate_directory},
};

pub(super) struct RepositoryIsolation {
    pub(super) runtime: EvalRuntime,
    run_home: Option<PathBuf>,
    isolation_root: Option<PathBuf>,
    pub(super) home: Option<PathBuf>,
    cleanup_pending: bool,
}

impl RepositoryIsolation {
    pub(super) fn prepare(
        runtime: &EvalRuntime,
        run_home: &Path,
        repo_name: &str,
        repo_config: &Value,
    ) -> Result<Self, String> {
        if !repo_config
            .get("isolated_index_home")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(Self {
                runtime: runtime.clone(),
                run_home: None,
                isolation_root: None,
                home: None,
                cleanup_pending: false,
            });
        }
        validate_directory(run_home, "evaluation home")?;
        let isolation_root = run_home.join("isolated-index-homes");
        match fs::create_dir(&isolation_root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "failed to create isolated repository root {}: {error}",
                    isolation_root.display()
                ));
            }
        }
        validate_direct_child(run_home, &isolation_root, "isolated repository root")?;
        let home = isolated_repository_home(&isolation_root, repo_name)?;
        match fs::create_dir(&home) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!(
                    "refusing to reuse existing isolated repository home {}",
                    home.display()
                ));
            }
            Err(error) => return Err(format!("failed to create {}: {error}", home.display())),
        }
        if let Err(error) =
            validate_direct_child(&isolation_root, &home, "isolated repository home")
        {
            let _ = fs::remove_dir(&home);
            return Err(error);
        }
        let mut isolated = runtime.clone();
        isolated.env.insert(
            "RELAY_KNOWLEDGE_HOME".to_owned(),
            home.display().to_string(),
        );
        eprintln!(
            "[self-iterate] repository isolated index home name={} home={} keep_workdirs={}",
            repo_name,
            home.display(),
            isolated.keep_workdirs
        );
        Ok(Self {
            runtime: isolated,
            run_home: Some(run_home.to_path_buf()),
            isolation_root: Some(isolation_root),
            home: Some(home),
            cleanup_pending: true,
        })
    }

    pub(super) fn complete<T>(mut self, result: Result<T, String>) -> Result<T, String> {
        let cleanup = self.cleanup();
        self.cleanup_pending = false;
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
            (Err(original), Ok(())) => Err(original),
            (Err(original), Err(cleanup_error)) => Err(format!(
                "{original}; additionally failed to clean isolated repository home: {cleanup_error}"
            )),
        }
    }

    fn cleanup(&self) -> Result<(), String> {
        if self.runtime.keep_workdirs {
            return Ok(());
        }
        match (&self.run_home, &self.isolation_root, &self.home) {
            (Some(run_home), Some(isolation_root), Some(home)) => {
                remove_isolated_repository_home(run_home, isolation_root, home)
            }
            (None, None, None) => Ok(()),
            _ => Err("isolated repository cleanup state is inconsistent".to_owned()),
        }
    }
}

impl Drop for RepositoryIsolation {
    fn drop(&mut self) {
        if self.cleanup_pending {
            if let Err(error) = self.cleanup() {
                eprintln!("[self-iterate] isolated repository cleanup failed: {error}");
            }
        }
    }
}

pub(super) fn isolated_repository_home(
    isolation_root: &Path,
    repo_name: &str,
) -> Result<PathBuf, String> {
    if repo_name.is_empty()
        || !repo_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!(
            "isolated repository name must be a safe path component: {repo_name:?}"
        ));
    }
    Ok(isolation_root.join(repo_name))
}

pub(super) fn remove_isolated_repository_home(
    run_home: &Path,
    isolation_root: &Path,
    home: &Path,
) -> Result<(), String> {
    if isolation_root.file_name().and_then(|name| name.to_str()) != Some("isolated-index-homes")
        || isolation_root.parent() != Some(run_home)
        || home.parent() != Some(isolation_root)
        || home
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| {
                isolated_repository_home(isolation_root, name).as_deref() != Ok(home)
            })
    {
        return Err(format!(
            "refusing to remove unsafe isolated repository home {} outside {}",
            home.display(),
            isolation_root.display()
        ));
    }
    validate_directory(run_home, "evaluation home")?;
    validate_direct_child(run_home, isolation_root, "isolated repository root")?;
    let metadata = match fs::symlink_metadata(home) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("failed to inspect {}: {error}", home.display())),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to recursively remove non-directory isolated repository home {}",
            home.display()
        ));
    }
    validate_direct_child(isolation_root, home, "isolated repository home")?;
    fs::remove_dir_all(home)
        .map_err(|error| format!("failed to remove {}: {error}", home.display()))
}

#[cfg(test)]
#[path = "isolation_tests.rs"]
mod tests;
