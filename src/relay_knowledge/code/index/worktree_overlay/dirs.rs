use std::{
    fs,
    path::{Path, PathBuf},
};

use super::super::MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS;
use crate::code::CodeIndexError;

pub(super) fn worktree_directory_files(
    root: &Path,
    relative_dir: &str,
) -> Result<Vec<String>, CodeIndexError> {
    if !worktree_directory_is_expandable(root, relative_dir)? {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_worktree_directory_files(root, Path::new(relative_dir), &mut files)?;
    files.sort();

    Ok(files)
}

pub(super) fn worktree_directory_is_expandable(
    root: &Path,
    relative_dir: &str,
) -> Result<bool, CodeIndexError> {
    let full_path = root.join(relative_dir);
    let metadata = fs::symlink_metadata(&full_path)?;
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }

    Ok(!contains_git_metadata(root, Path::new(relative_dir))?)
}

fn collect_worktree_directory_files(
    root: &Path,
    relative: &Path,
    files: &mut Vec<String>,
) -> Result<(), CodeIndexError> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if entry.file_name() == ".git" || contains_git_metadata(root, &path)? {
                continue;
            }
            collect_worktree_directory_files(root, &path, files)?;
        } else if file_type.is_file() {
            record_directory_file(path, files)?;
        }
    }

    Ok(())
}

fn record_directory_file(path: PathBuf, files: &mut Vec<String>) -> Result<(), CodeIndexError> {
    if files.len() >= MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "untracked worktree directory expands past {MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS} files; run a full code index or narrow --path before indexing --ref worktree"
        )));
    }
    files.push(path.to_string_lossy().replace('\\', "/"));
    Ok(())
}

fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
