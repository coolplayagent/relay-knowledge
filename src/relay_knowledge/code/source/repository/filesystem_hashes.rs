use std::{collections::BTreeMap, path::Path};

use super::super::{
    CodeIndexError,
    ids::{stable_content_hash, stable_hash64},
};
use super::filesystem_access::filesystem_bytes;

const FILESYSTEM_SYNTHETIC_PREFIX: &str = "filesystem:";

pub(in crate::code) fn source_commit_is_filesystem(commit: &str) -> bool {
    commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX)
}

pub(in crate::code) fn filesystem_tree_hash_for_paths(
    root: &Path,
    paths: &[String],
) -> Result<String, CodeIndexError> {
    let path_hashes = filesystem_content_hashes_for_paths(root, paths)?;

    Ok(filesystem_tree_hash_from_path_hashes(&path_hashes))
}

pub(in crate::code) fn filesystem_content_hashes_for_paths(
    root: &Path,
    paths: &[String],
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    let root = root.canonicalize()?;
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut path_hashes = BTreeMap::new();
    for path in paths {
        path_hashes.insert(path.clone(), filesystem_content_hash(&root, &path)?);
    }

    Ok(path_hashes)
}

pub(in crate::code) fn filesystem_tree_hash_from_path_hashes(
    path_hashes: &BTreeMap<String, String>,
) -> String {
    let mut hash_input = Vec::new();
    for (path, content_hash) in path_hashes {
        hash_input.extend_from_slice(path.as_bytes());
        hash_input.push(0);
        hash_input.extend_from_slice(content_hash.as_bytes());
        hash_input.push(0);
    }

    format!(
        "{FILESYSTEM_SYNTHETIC_PREFIX}{:016x}",
        stable_hash64(&hash_input)
    )
}

pub(in crate::code) fn ensure_filesystem_paths_match_content_hashes(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<(), CodeIndexError> {
    if !source_commit_is_filesystem(commit) {
        return Ok(());
    }
    let root = root.canonicalize()?;
    for path in paths {
        let expected_hash = expected_hashes.get(path).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing planned content hash for {path}"
            ))
        })?;
        let actual_hash = filesystem_content_hash(&root, path)?;
        if &actual_hash != expected_hash {
            return Err(CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} no longer matches planned filesystem file {path}"
            )));
        }
    }

    Ok(())
}

pub(in crate::code) fn ensure_filesystem_blobs_match_content_hashes(
    commit: &str,
    paths: &[String],
    blobs: &[Vec<u8>],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<(), CodeIndexError> {
    if !source_commit_is_filesystem(commit) {
        return Ok(());
    }
    for (path, bytes) in paths.iter().zip(blobs) {
        let expected_hash = expected_hashes.get(path).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing planned content hash for {path}"
            ))
        })?;
        let actual_hash = stable_content_hash(bytes);
        if &actual_hash != expected_hash {
            return Err(CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} no longer matches planned filesystem file {path}"
            )));
        }
    }

    Ok(())
}

fn filesystem_content_hash(root: &Path, path: &str) -> Result<String, CodeIndexError> {
    filesystem_bytes(root, path)
        .map(|bytes| stable_content_hash(&bytes))
        .map_err(|error| match error {
            CodeIndexError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                CodeIndexError::InvalidInput(format!(
                    "filesystem source path {path} is missing from live source tree"
                ))
            }
            error => error,
        })
}

#[cfg(test)]
#[path = "filesystem_hashes_tests.rs"]
mod tests;
