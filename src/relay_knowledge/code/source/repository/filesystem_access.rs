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
    fs::read(safe_filesystem_path(root, path)?).map_err(CodeIndexError::Io)
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
) -> Result<Vec<FileSystemFile>, CodeIndexError> {
    if policy.path_scope_denied {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files(root, Path::new(""), policy, &mut files)?;
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
        match fs::symlink_metadata(&checked_path) {
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
    match fs::symlink_metadata(&full_path) {
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
) -> Result<(), CodeIndexError> {
    let mut entries = fs::read_dir(root.join(relative))?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let directory = path.to_string_lossy().replace('\\', "/");
            if directory_is_excluded(&path, policy)
                || !policy.should_descend_directory(&directory)
                || contains_git_metadata(root, &path)?
            {
                continue;
            }
            collect_files(root, &path, policy, files)?;
            continue;
        }
        if !file_type.is_file() {
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
