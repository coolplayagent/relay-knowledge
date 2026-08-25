use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
use std::{fs, path::PathBuf, sync::Mutex};

use super::super::{
    CodeIndexError,
    git::{git_batch_blob_sizes, git_batch_blobs, git_batch_blobs_without_fallback, git_bytes},
    source_gitlink,
};
use super::{
    filesystem_access::{filesystem_blob_sizes, filesystem_bytes},
    filesystem_hashes::{
        ensure_filesystem_blobs_match_content_hashes, source_commit_is_filesystem,
    },
    identity::RepositorySourceKind,
};

#[cfg(test)]
struct FileSystemPolicyReadMutation {
    root: PathBuf,
    path: String,
    content: Vec<u8>,
}

#[cfg(test)]
static FILESYSTEM_POLICY_READ_MUTATION: Mutex<Option<FileSystemPolicyReadMutation>> =
    Mutex::new(None);

#[cfg(test)]
#[derive(Debug, Default)]
struct SourceReadCounts {
    single_reads: usize,
    batch_reads: usize,
}

#[cfg(test)]
const MAX_SOURCE_READ_OBSERVERS: usize = 64;

#[cfg(test)]
static SOURCE_READ_OBSERVERS: Mutex<BTreeMap<PathBuf, SourceReadCounts>> =
    Mutex::new(BTreeMap::new());

#[cfg(test)]
pub(crate) fn mutate_next_filesystem_policy_read(root: PathBuf, path: &str, content: &[u8]) {
    *FILESYSTEM_POLICY_READ_MUTATION
        .lock()
        .expect("filesystem read mutation should lock") = Some(FileSystemPolicyReadMutation {
        root,
        path: path.to_owned(),
        content: content.to_vec(),
    });
}

fn source_bytes_after_policy_verification(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    record_source_single_read(root);
    match git_bytes(root, ["show", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(bytes),
        Err(error) => source_gitlink::submodule_bytes(root, commit, path).map_err(|_| error),
    }
}

pub(in crate::code) fn source_bytes_after_content_verification(
    root: &Path,
    commit: &str,
    path: &str,
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<Vec<u8>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        let paths = [path.to_owned()];
        let blobs =
            source_batch_bytes_after_content_verification(root, commit, &paths, expected_hashes)?;
        return blobs.into_iter().next().ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} produced no bytes for {path}"
            ))
        });
    }

    source_bytes_after_policy_verification(root, commit, path)
}

pub(in crate::code) fn source_snapshot_bytes(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    if kind.is_filesystem() {
        return filesystem_bytes(root, path);
    }

    source_bytes_after_policy_verification(root, commit, path)
}

fn source_batch_bytes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    record_source_batch_read(root);
    if let Ok(blobs) = git_batch_blobs_without_fallback(root, commit, paths) {
        return Ok(blobs);
    }
    let sizes = match git_batch_blob_sizes(root, commit, paths) {
        Ok(sizes) => sizes,
        Err(_) => {
            return paths
                .iter()
                .map(|path| source_bytes_after_policy_verification(root, commit, path))
                .collect();
        }
    };

    let mut blobs = vec![None::<Vec<u8>>; paths.len()];
    let mut parent_blob_indices = Vec::new();
    let mut parent_blob_paths = Vec::new();
    for (index, (path, size)) in paths.iter().zip(sizes.iter()).enumerate() {
        if size.is_some() {
            parent_blob_indices.push(index);
            parent_blob_paths.push(path.clone());
        }
    }

    let parent_blobs = if parent_blob_paths.is_empty() {
        Vec::new()
    } else {
        match git_batch_blobs(root, commit, &parent_blob_paths) {
            Ok(blobs) => blobs,
            Err(_) => parent_blob_paths
                .iter()
                .map(|path| source_bytes_after_policy_verification(root, commit, path))
                .collect::<Result<Vec<_>, _>>()?,
        }
    };
    for (index, bytes) in parent_blob_indices.into_iter().zip(parent_blobs) {
        blobs[index] = Some(bytes);
    }
    for (index, path) in paths.iter().enumerate() {
        if blobs[index].is_none() {
            blobs[index] = Some(source_bytes_after_policy_verification(root, commit, path)?);
        }
    }

    blobs
        .into_iter()
        .map(|bytes| {
            bytes.ok_or_else(|| {
                CodeIndexError::InvalidInput(
                    "source batch bytes left a path without content".to_owned(),
                )
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn reset_source_read_counts_for_root(root: PathBuf) {
    let root = source_read_observer_key(&root);
    let mut observers = SOURCE_READ_OBSERVERS
        .lock()
        .expect("source read observers should lock");
    if !observers.contains_key(&root) && observers.len() >= MAX_SOURCE_READ_OBSERVERS {
        observers.pop_first();
    }
    observers.insert(root, SourceReadCounts::default());
}

#[cfg(test)]
pub(crate) fn source_read_counts_for_root(root: &Path) -> (usize, usize) {
    SOURCE_READ_OBSERVERS
        .lock()
        .expect("source read observers should lock")
        .get(&source_read_observer_key(root))
        .map(|counts| (counts.single_reads, counts.batch_reads))
        .unwrap_or_default()
}

#[cfg(test)]
fn source_read_observer_key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

fn record_source_single_read(_root: &Path) {
    #[cfg(test)]
    if let Some(counts) = SOURCE_READ_OBSERVERS
        .lock()
        .expect("source read observers should lock")
        .get_mut(&source_read_observer_key(_root))
    {
        counts.single_reads += 1;
    }
}

fn record_source_batch_read(_root: &Path) {
    #[cfg(test)]
    if let Some(counts) = SOURCE_READ_OBSERVERS
        .lock()
        .expect("source read observers should lock")
        .get_mut(&source_read_observer_key(_root))
    {
        counts.batch_reads += 1;
    }
}

pub(in crate::code) fn source_batch_bytes_after_content_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        let expected_hashes = expected_hashes.ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing verified content hashes"
            ))
        })?;
        return filesystem_batch_bytes_after_hash_check(root, commit, paths, expected_hashes);
    }

    source_batch_bytes_after_policy_verification(root, commit, paths)
}

pub(in crate::code) fn source_snapshot_batch_bytes(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if kind.is_filesystem() {
        return paths
            .iter()
            .map(|path| filesystem_bytes(root, path))
            .collect();
    }

    source_batch_bytes_after_policy_verification(root, commit, paths)
}

pub(in crate::code) fn source_blob_sizes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        return filesystem_blob_sizes(root, paths);
    }

    let mut sizes = match git_batch_blob_sizes(root, commit, paths) {
        Ok(sizes) => sizes,
        Err(_) => {
            return paths
                .iter()
                .map(|path| git_blob_size_after_policy_verification(root, commit, path))
                .collect();
        }
    };
    for (path, size) in paths.iter().zip(sizes.iter_mut()) {
        if size.is_none() {
            *size = source_gitlink::submodule_blob_size(root, commit, path)?;
        }
    }

    Ok(sizes)
}

fn git_blob_size_after_policy_verification(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    let object = format!("{commit}:{path}");
    match git_bytes(root, ["cat-file", "-s", &object]) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
        Err(_) => source_gitlink::submodule_blob_size(root, commit, path),
    }
}

fn filesystem_batch_bytes_after_hash_check(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    #[cfg(test)]
    apply_filesystem_policy_read_mutation(root)?;
    let blobs = paths
        .iter()
        .map(|path| filesystem_bytes(root, path))
        .collect::<Result<Vec<_>, _>>()?;
    ensure_filesystem_blobs_match_content_hashes(commit, paths, &blobs, expected_hashes)?;

    Ok(blobs)
}

#[cfg(test)]
fn apply_filesystem_policy_read_mutation(root: &Path) -> Result<(), CodeIndexError> {
    let mut mutation = FILESYSTEM_POLICY_READ_MUTATION
        .lock()
        .expect("filesystem read mutation should lock");
    let Some(next) = mutation.take() else {
        return Ok(());
    };
    if next.root != root {
        *mutation = Some(next);
        return Ok(());
    }

    fs::write(root.join(next.path), next.content).map_err(CodeIndexError::Io)
}

#[cfg(test)]
#[path = "blobs_tests.rs"]
mod tests;
