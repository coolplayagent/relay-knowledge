//! Expands changed directories with bounded traversal and nested-repository exclusion.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::super::MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS;
use crate::code::{CodeIndexError, source::path_io::SkippedSourcePath};
use crate::domain::{CodePathIoOperation, CodePathKind};

pub(super) fn worktree_directory_files(
    root: &Path,
    relative_dir: &str,
    skipped: &mut Vec<SkippedSourcePath>,
    selected: &impl Fn(&str, bool) -> bool,
) -> Result<Vec<String>, CodeIndexError> {
    if !worktree_directory_is_expandable(root, relative_dir)? {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_worktree_directory_files(root, Path::new(relative_dir), &mut files, skipped, selected)?;
    files.sort();

    Ok(files)
}

pub(super) fn worktree_directory_is_expandable(
    root: &Path,
    relative_dir: &str,
) -> Result<bool, CodeIndexError> {
    let full_path = root.join(relative_dir);
    let metadata = crate::code::source::local_io::symlink_metadata(&full_path)?;
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }

    Ok(!contains_git_metadata(root, Path::new(relative_dir))?)
}

fn collect_worktree_directory_files(
    root: &Path,
    relative: &Path,
    files: &mut Vec<String>,
    skipped: &mut Vec<SkippedSourcePath>,
    selected: &impl Fn(&str, bool) -> bool,
) -> Result<(), CodeIndexError> {
    let relative_name = relative.to_string_lossy().replace('\\', "/");
    if !selected(&relative_name, true) {
        return Ok(());
    }
    let mut entries = match crate::code::source::local_io::read_directory(&root.join(relative)) {
        Ok(entries) => entries,
        Err(error) => {
            skipped.push(SkippedSourcePath::from_error(
                &relative_name,
                CodePathKind::Directory,
                CodePathIoOperation::ReadDirectory,
                error,
            )?);
            return Ok(());
        }
    };
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() > MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(
            "worktree directory exceeds bounded enumeration budget".into(),
        ));
    }
    let initial_file_count = files.len();
    let initial_skip_count = skipped.len();
    for entry in entries {
        let path = relative.join(entry.file_name());
        let name = path.to_string_lossy().replace('\\', "/");
        if !selected(&name, false) && !selected(&name, true) {
            continue;
        }
        let file_type = match crate::code::source::local_io::directory_entry_type(&entry) {
            Ok(kind) => kind,
            Err(error) => {
                let failure = SkippedSourcePath::from_error(
                    &relative_name,
                    CodePathKind::Directory,
                    CodePathIoOperation::Metadata,
                    error,
                )?;
                files.truncate(initial_file_count);
                skipped.truncate(initial_skip_count);
                skipped.push(failure);
                return Ok(());
            }
        };
        if file_type.is_dir() && entry.file_name() != ".git" && selected(&name, true) {
            match contains_git_metadata(root, &path) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(CodeIndexError::Io(error)) => {
                    skipped.push(SkippedSourcePath::from_error(
                        &name,
                        CodePathKind::Directory,
                        CodePathIoOperation::CheckRepositoryBoundary,
                        error,
                    )?);
                    continue;
                }
                Err(error) => return Err(error),
            }
            collect_worktree_directory_files(root, &path, files, skipped, selected)?;
        } else if file_type.is_file() && selected(&name, false) {
            record_directory_file(path, files)?;
        } else if !file_type.is_dir() && selected(&name, false) {
            skipped.push(SkippedSourcePath::from_error(
                &name,
                CodePathKind::File,
                CodePathIoOperation::Metadata,
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "source is not a regular file",
                ),
            )?);
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

pub(super) fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
