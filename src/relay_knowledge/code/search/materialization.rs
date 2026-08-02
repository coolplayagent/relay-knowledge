use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    code::{
        CodeIndexError, generated_detection,
        scope::{
            scoped_source_snapshot_for_registration,
            scoped_source_snapshot_for_registration_filters,
        },
        source::{
            source_batch_bytes_after_content_verification,
            source_blob_sizes_after_policy_verification, source_bytes_after_content_verification,
            source_commit_is_filesystem,
        },
    },
    domain::CodeRepositoryRegistration,
};

const MAX_GREP_BYTES: usize = 8 * 1024 * 1024;
const GENERATED_EXCLUSION_READ_BUDGET_MULTIPLIER: usize = 4;
const WORKTREE_OVERLAY_FALLBACK_SCOPE: &str = "filesystem:worktree-overlay-fallback";

pub(super) struct MaterializedFiles {
    pub(super) file_count: usize,
    pub(super) degraded_reason: Option<String>,
}

pub(super) struct TempSourceTree {
    pub(super) root: PathBuf,
}

pub(super) fn materialize_source_blobs(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    paths: &[String],
    path_filters: &[String],
    language_filters: &[String],
    exclude_generated: bool,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    materialize_source_blobs_at_root(
        registration,
        &root,
        commit,
        paths,
        SourceMaterializationOptions {
            path_filters,
            language_filters,
            exclude_generated,
            max_bytes: MAX_GREP_BYTES,
        },
        tree,
    )
}

pub(super) fn materialize_worktree_overlay_source_blobs(
    registration: &CodeRepositoryRegistration,
    paths: &[String],
    tree: &mut TempSourceTree,
    expected_hashes: &BTreeMap<String, String>,
    exclude_generated: bool,
) -> Result<MaterializedFiles, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    materialize_source_blobs_per_path(
        &root,
        WORKTREE_OVERLAY_FALLBACK_SCOPE,
        paths,
        tree,
        MAX_GREP_BYTES,
        Some(expected_hashes),
        exclude_generated,
    )
}

#[derive(Clone, Copy)]
struct SourceMaterializationOptions<'a> {
    path_filters: &'a [String],
    language_filters: &'a [String],
    exclude_generated: bool,
    max_bytes: usize,
}

fn materialize_source_blobs_at_root(
    registration: &CodeRepositoryRegistration,
    root: &Path,
    commit: &str,
    paths: &[String],
    options: SourceMaterializationOptions<'_>,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let verified_hashes = match ensure_source_grep_commit_current(
        registration,
        commit,
        options.path_filters,
        options.language_filters,
    ) {
        Ok(hashes) => hashes,
        Err(error) if source_commit_is_filesystem(commit) => {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        Err(error) => return Err(error),
    };
    let materialized = if options.exclude_generated {
        materialize_source_blobs_per_path(
            root,
            commit,
            paths,
            tree,
            options.max_bytes,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    } else if let Some(selection) =
        candidate_source_blob_selection(root, commit, paths, options.max_bytes)
    {
        materialize_selected_source_blobs(
            root,
            commit,
            selection,
            tree,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    } else {
        materialize_source_blobs_per_path(
            root,
            commit,
            paths,
            tree,
            options.max_bytes,
            verified_hashes.as_ref(),
            options.exclude_generated,
        )?
    };
    if let Err(error) = ensure_source_grep_commit_current(
        registration,
        commit,
        options.path_filters,
        options.language_filters,
    ) {
        if source_commit_is_filesystem(commit) {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        return Err(error);
    }

    Ok(materialized)
}

fn ensure_source_grep_commit_current(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Option<BTreeMap<String, String>>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        return filesystem_hashes_for_verified_scope(
            registration,
            commit,
            path_filters,
            language_filters,
        )
        .map(Some);
    }

    Ok(None)
}

fn filesystem_hashes_for_verified_scope(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    match scoped_source_snapshot_for_registration(registration, commit) {
        Ok(snapshot) => Ok(snapshot.content_hashes),
        Err(stored_scope_error) => {
            match scoped_source_snapshot_for_registration_filters(
                registration,
                commit,
                path_filters,
                language_filters,
            ) {
                Ok(snapshot) => Ok(snapshot.content_hashes),
                Err(_) => Err(stored_scope_error),
            }
        }
    }
}

fn materialize_selected_source_blobs(
    root: &Path,
    commit: &str,
    selection: BlobCandidateSelection,
    tree: &mut TempSourceTree,
    expected_hashes: Option<&BTreeMap<String, String>>,
    exclude_generated: bool,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut file_count = 0usize;
    for (path, bytes) in selection.paths.iter().zip(candidate_source_blobs(
        root,
        commit,
        &selection.paths,
        expected_hashes,
    )) {
        let Some(bytes) = bytes else {
            continue;
        };
        if exclude_generated && generated_detection::is_generated_file(path, &bytes) {
            continue;
        }
        tree.write(path, bytes.as_slice())?;
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: selection
            .exhausted
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

fn materialize_source_blobs_per_path(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
    max_bytes: usize,
    expected_hashes: Option<&BTreeMap<String, String>>,
    exclude_generated: bool,
) -> Result<MaterializedFiles, CodeIndexError> {
    let sizes = source_blob_sizes_after_policy_verification(root, commit, paths).ok();
    let mut budget = SourceMaterializationBudget::new(max_bytes, exclude_generated);
    let mut file_count = 0usize;
    for (index, path) in paths.iter().enumerate() {
        if let Some(size) = sizes
            .as_ref()
            .and_then(|sizes| sizes.get(index))
            .copied()
            .flatten()
            && !budget.may_read_known_size(size)
        {
            continue;
        }
        let Ok(bytes) =
            source_bytes_after_content_verification(root, commit, path, expected_hashes)
        else {
            continue;
        };
        budget.record_read(bytes.len());
        if bytes.len() > max_bytes {
            budget.mark_exhausted();
            continue;
        }
        if exclude_generated && generated_detection::is_generated_file(path, &bytes) {
            continue;
        }
        if !budget.try_materialize(bytes.len()) {
            continue;
        }
        tree.write(path, &bytes)?;
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: budget
            .is_exhausted()
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

struct SourceMaterializationBudget {
    materialized_bytes: usize,
    read_bytes: usize,
    materialized_limit: usize,
    read_limit: usize,
    exclude_generated: bool,
    exhausted: bool,
}

impl SourceMaterializationBudget {
    fn new(materialized_limit: usize, exclude_generated: bool) -> Self {
        let read_limit = if exclude_generated {
            materialized_limit.saturating_mul(GENERATED_EXCLUSION_READ_BUDGET_MULTIPLIER)
        } else {
            materialized_limit
        };
        Self {
            materialized_bytes: 0,
            read_bytes: 0,
            materialized_limit,
            read_limit,
            exclude_generated,
            exhausted: false,
        }
    }

    fn may_read_known_size(&mut self, size: usize) -> bool {
        if size > self.materialized_limit
            || self.read_bytes.saturating_add(size) > self.read_limit
            || (!self.exclude_generated
                && self.materialized_bytes.saturating_add(size) > self.materialized_limit)
        {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn record_read(&mut self, size: usize) {
        self.read_bytes = self.read_bytes.saturating_add(size);
        if self.read_bytes > self.read_limit {
            self.exhausted = true;
        }
    }

    fn try_materialize(&mut self, size: usize) -> bool {
        if self.materialized_bytes.saturating_add(size) > self.materialized_limit {
            self.exhausted = true;
            return false;
        }
        self.materialized_bytes += size;
        true
    }

    fn mark_exhausted(&mut self) {
        self.exhausted = true;
    }

    fn is_exhausted(&self) -> bool {
        self.exhausted
    }
}

struct BlobCandidateSelection {
    paths: Vec<String>,
    exhausted: bool,
}

fn candidate_source_blob_selection(
    root: &Path,
    commit: &str,
    paths: &[String],
    max_bytes: usize,
) -> Option<BlobCandidateSelection> {
    let sizes = source_blob_sizes_after_policy_verification(root, commit, paths).ok()?;
    if sizes.len() != paths.len() {
        return None;
    }

    let mut selected_paths = Vec::new();
    let mut byte_count = 0usize;
    let mut exhausted = false;
    for (path, size) in paths.iter().zip(sizes) {
        let Some(size) = size else {
            continue;
        };
        if byte_count.saturating_add(size) > max_bytes {
            exhausted = true;
            continue;
        }
        selected_paths.push(path.clone());
        byte_count += size;
    }

    Some(BlobCandidateSelection {
        paths: selected_paths,
        exhausted,
    })
}

fn candidate_source_blobs(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Vec<Option<Vec<u8>>> {
    match source_batch_bytes_after_content_verification(root, commit, paths, expected_hashes) {
        Ok(blobs) if blobs.len() == paths.len() => blobs.into_iter().map(Some).collect(),
        Err(_) if source_commit_is_filesystem(commit) => paths.iter().map(|_| None).collect(),
        _ => paths
            .iter()
            .map(|path| {
                source_bytes_after_content_verification(root, commit, path, expected_hashes).ok()
            })
            .collect(),
    }
}

impl TempSourceTree {
    pub(super) fn create() -> Result<Self, CodeIndexError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-source-grep-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;

        Ok(Self { root })
    }

    pub(super) fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), CodeIndexError> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, bytes)?;

        Ok(())
    }
}

impl Drop for TempSourceTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
#[path = "materialization_tests.rs"]
mod tests;
