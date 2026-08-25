use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::history::HistoryPaths;

const EVALUATION_HOME_COMPONENT: &str = "home";

#[derive(Debug)]
pub(in crate::evaluator) struct EvaluationHome {
    work_root: PathBuf,
    home: PathBuf,
    keep_workdirs: bool,
    cleanup_pending: bool,
}

impl EvaluationHome {
    pub(in crate::evaluator) fn prepare(
        paths: &HistoryPaths,
        run_id: &str,
        keep_workdirs: bool,
    ) -> Result<Self, String> {
        validate_component(run_id, "run id")?;
        validate_directory(&paths.work, "self-iteration work root")?;
        let run_root = paths.work.join(run_id);
        match fs::create_dir(&run_root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!(
                    "refusing to reuse existing self-iteration run directory {}",
                    run_root.display()
                ));
            }
            Err(error) => {
                return Err(format!(
                    "failed to create self-iteration run directory {}: {error}",
                    run_root.display()
                ));
            }
        }
        if let Err(error) = validate_direct_child(&paths.work, &run_root, "evaluation run root") {
            let _ = fs::remove_dir(&run_root);
            return Err(error);
        }
        let home = run_root.join(EVALUATION_HOME_COMPONENT);
        if let Err(error) = fs::create_dir(&home) {
            let _ = fs::remove_dir(&run_root);
            return Err(format!(
                "failed to create evaluation home {}: {error}",
                home.display()
            ));
        }
        if let Err(error) = validate_direct_child(&run_root, &home, "evaluation home") {
            let _ = fs::remove_dir(&home);
            let _ = fs::remove_dir(&run_root);
            return Err(error);
        }
        Ok(Self {
            work_root: paths.work.clone(),
            home,
            keep_workdirs,
            cleanup_pending: true,
        })
    }

    pub(in crate::evaluator) fn path(&self) -> &Path {
        &self.home
    }

    pub(in crate::evaluator) fn complete_result<T>(
        mut self,
        result: Result<T, String>,
    ) -> Result<T, String> {
        let cleanup = self.cleanup();
        self.cleanup_pending = false;
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
            (Err(original), Ok(())) => Err(original),
            (Err(original), Err(cleanup_error)) => Err(format!(
                "{original}; additionally failed to clean evaluation home: {cleanup_error}"
            )),
        }
    }

    fn cleanup(&self) -> Result<(), String> {
        if self.keep_workdirs {
            return Ok(());
        }
        remove_evaluation_home(&self.work_root, &self.home)
    }
}

impl Drop for EvaluationHome {
    fn drop(&mut self) {
        if !self.cleanup_pending {
            return;
        }
        if let Err(error) = self.cleanup() {
            eprintln!("[self-iterate] evaluation home cleanup failed: {error}");
        }
    }
}

pub(in crate::evaluator) fn remove_evaluation_home(
    work_root: &Path,
    home: &Path,
) -> Result<(), String> {
    let Some(run_root) = home.parent() else {
        return Err(format!(
            "refusing to remove evaluation home without a run root: {}",
            home.display()
        ));
    };
    if home.file_name().and_then(|name| name.to_str()) != Some(EVALUATION_HOME_COMPONENT)
        || run_root.parent() != Some(work_root)
    {
        return Err(format!(
            "refusing to remove evaluation home {} outside work root {}",
            home.display(),
            work_root.display()
        ));
    }
    validate_directory(work_root, "self-iteration work root")?;
    validate_direct_child(work_root, run_root, "evaluation run root")?;
    validate_direct_child(run_root, home, "evaluation home")?;
    fs::remove_dir_all(run_root)
        .map_err(|error| format!("failed to remove {}: {error}", run_root.display()))
}

pub(in crate::evaluator) fn validate_direct_child(
    parent: &Path,
    child: &Path,
    label: &str,
) -> Result<(), String> {
    if child.parent() != Some(parent) {
        return Err(format!(
            "refusing {label} {} outside direct parent {}",
            child.display(),
            parent.display()
        ));
    }
    validate_directory(parent, "parent directory")?;
    validate_directory(child, label)?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| format!("failed to canonicalize {}: {error}", parent.display()))?;
    let canonical_child = fs::canonicalize(child)
        .map_err(|error| format!("failed to canonicalize {}: {error}", child.display()))?;
    if canonical_child.parent() != Some(canonical_parent.as_path()) {
        return Err(format!(
            "refusing {label} {} whose canonical path escapes {}",
            child.display(),
            parent.display()
        ));
    }
    Ok(())
}

pub(in crate::evaluator) fn validate_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "refusing {label} {} because it is not a non-symlink directory",
            path.display()
        ));
    }
    Ok(())
}

fn validate_component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!("{label} must be a safe path component: {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "workdir_tests.rs"]
mod tests;
