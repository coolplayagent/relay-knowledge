use std::{
    fs,
    path::{Path, PathBuf},
};

use super::super::{
    CodeIndexError, filesystem::FileSystemScanPolicy, source_paths::FILESYSTEM_BROAD_SEGMENTS,
};
use super::normalize_path_filter;

#[derive(Debug, Clone)]
pub(super) struct FileSystemFile {
    pub(super) path: String,
}

pub(super) fn filesystem_bytes(root: &Path, path: &str) -> Result<Vec<u8>, CodeIndexError> {
    crate::code::source::local_io::read_file(&safe_filesystem_path(root, path)?)
        .map_err(CodeIndexError::Io)
}

pub(super) fn filesystem_blob_sizes(
    root: &Path,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    paths
        .iter()
        .map(|path| {
            let full_path = safe_filesystem_path(root, path)?;
            Ok(fs::metadata(full_path)
                .ok()
                .map(|metadata| usize::try_from(metadata.len()).unwrap_or(usize::MAX)))
        })
        .collect()
}

pub(super) fn filesystem_byte_count(root: &Path, path: &str) -> Result<usize, CodeIndexError> {
    fs::metadata(safe_filesystem_path(root, path)?)
        .map(|metadata| usize::try_from(metadata.len()).unwrap_or(usize::MAX))
        .map_err(CodeIndexError::Io)
}

pub(super) fn filesystem_files(
    root: &Path,
    policy: &FileSystemScanPolicy,
    skipped: &mut Vec<crate::code::source::path_io::SkippedSourcePath>,
) -> Result<Vec<FileSystemFile>, CodeIndexError> {
    if policy.path_scope_denied {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files(root, Path::new(""), policy, &mut files, skipped)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|left, right| left.path == right.path);

    Ok(files)
}

fn safe_filesystem_path(root: &Path, path: &str) -> Result<PathBuf, CodeIndexError> {
    if !safe_relative_path(path) {
        return Err(CodeIndexError::InvalidInput(format!(
            "unsafe repository source path '{path}'"
        )));
    }

    let mut checked_path = root.to_path_buf();
    let mut checked_relative = PathBuf::new();
    for component in Path::new(path).components() {
        checked_path.push(component.as_os_str());
        checked_relative.push(component.as_os_str());
        match crate::code::source::local_io::symlink_metadata(&checked_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CodeIndexError::InvalidInput(format!(
                    "filesystem source path {path} component {} is a symlink and is outside the authorized regular-file scope",
                    checked_relative.to_string_lossy()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(root.join(path));
            }
            Err(error) => return Err(CodeIndexError::Io(error)),
        }
    }

    let full_path = checked_path;
    match crate::code::source::local_io::symlink_metadata(&full_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CodeIndexError::InvalidInput(format!(
                "filesystem source path {path} is a symlink and is outside the authorized regular-file scope"
            )))
        }
        Ok(_) => Ok(full_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(full_path),
        Err(error) => Err(CodeIndexError::Io(error)),
    }
}

fn collect_files(
    root: &Path,
    relative: &Path,
    policy: &FileSystemScanPolicy,
    files: &mut Vec<FileSystemFile>,
    skipped: &mut Vec<crate::code::source::path_io::SkippedSourcePath>,
) -> Result<(), CodeIndexError> {
    let enumeration = crate::code::source::local_io::read_directory(&root.join(relative));
    let mut entries = match enumeration {
        Ok(entries) => entries,
        Err(error) => {
            skipped.push(crate::code::source::path_io::SkippedSourcePath::from_error(
                &relative.to_string_lossy().replace('\\', "/"),
                crate::domain::CodePathKind::Directory,
                crate::domain::CodePathIoOperation::ReadDirectory,
                error,
            )?);
            return Ok(());
        }
    };
    entries.sort_by_key(|entry| entry.file_name());
    let initial_file_count = files.len();
    let initial_skip_count = skipped.len();
    for entry in entries {
        let path = relative.join(entry.file_name());
        let relative_path = path.to_string_lossy().replace('\\', "/");
        let selected = policy.hash_includes_path(&relative_path)
            && policy.language_allows_hash(&relative_path)
            && policy.file_preset_allows_hash(&relative_path);
        if !selected && !policy.should_descend_directory(&relative_path) {
            continue;
        }
        let file_type = match crate::code::source::local_io::directory_entry_type(&entry) {
            Ok(kind) => kind,
            Err(error) => {
                // A selection predicate cannot establish the failed entry's type. Revoke
                // the complete known parent subtree, including earlier nested results.
                let failure = crate::code::source::path_io::SkippedSourcePath::from_error(
                    &relative.to_string_lossy().replace('\\', "/"),
                    crate::domain::CodePathKind::Directory,
                    crate::domain::CodePathIoOperation::Metadata,
                    error,
                )?;
                files.truncate(initial_file_count);
                skipped.truncate(initial_skip_count);
                skipped.push(failure);
                return Ok(());
            }
        };
        if file_type.is_dir() {
            let directory = path.to_string_lossy().replace('\\', "/");
            if directory_is_excluded(&path, policy) || !policy.should_descend_directory(&directory)
            {
                continue;
            }
            match contains_git_metadata(root, &path) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(CodeIndexError::Io(error)) => {
                    skipped.push(crate::code::source::path_io::SkippedSourcePath::from_error(
                        &relative_path,
                        crate::domain::CodePathKind::Directory,
                        crate::domain::CodePathIoOperation::CheckRepositoryBoundary,
                        error,
                    )?);
                    continue;
                }
                Err(error) => return Err(error),
            }
            collect_files(root, &path, policy, files, skipped)?;
            continue;
        }
        if !file_type.is_file() {
            if selected {
                skipped.push(crate::code::source::path_io::SkippedSourcePath::from_error(
                    &relative_path,
                    crate::domain::CodePathKind::File,
                    crate::domain::CodePathIoOperation::Metadata,
                    std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "source is not a regular file",
                    ),
                )?);
            }
            continue;
        }
        let path = path.to_string_lossy().replace('\\', "/");
        if safe_relative_path(&path) {
            files.push(FileSystemFile { path });
        }
    }

    Ok(())
}

fn directory_is_excluded(relative: &Path, policy: &FileSystemScanPolicy) -> bool {
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name == ".git" {
        return true;
    }
    if !FILESYSTEM_BROAD_SEGMENTS.contains(&name) {
        return false;
    }
    let directory = relative.to_string_lossy().replace('\\', "/");
    let directory = normalize_path_filter(&directory);

    !policy.includes_broad_directory(directory)
}

fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
#[path = "filesystem_access_tests.rs"]
mod tests;
