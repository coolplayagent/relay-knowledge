use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{
    code::{
        CodeIndexError, snapshot,
        source::{
            changes::GitTreeEntry,
            filesystem::FileSystemScanPolicy,
            repository::{
                RepositorySourceKind, RepositorySourceSnapshot,
                filesystem_content_hashes_for_paths, filesystem_registration_identity,
                filesystem_source_snapshot, filesystem_tree_hash_from_path_hashes,
                source_commit_is_filesystem, source_kind, source_snapshot,
            },
        },
    },
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::{
    discover_source_layout, effective_index_path_filters, intersect_path_filters,
    selection_exclusion_reason_for_source,
};

#[derive(Debug, Clone)]
pub(in crate::code) struct ScopedSourceSnapshot {
    pub(in crate::code) kind: RepositorySourceKind,
    pub(in crate::code) root: PathBuf,
    pub(in crate::code) resolved_commit_sha: String,
    pub(in crate::code) tree_hash: String,
    pub(in crate::code) entries: Vec<GitTreeEntry>,
    pub(in crate::code) content_hashes: BTreeMap<String, String>,
    pub(in crate::code) path_filters: Vec<String>,
    pub(in crate::code) language_filters: Vec<String>,
}

pub(in crate::code) fn scoped_source_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    ref_selector: &str,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let allow_filesystem_ref =
        registration_allows_filesystem_ref(registration, root, ref_selector)?;
    scoped_source_snapshot_inner(
        registration,
        selector,
        root,
        ref_selector,
        allow_filesystem_ref,
    )
}

pub(in crate::code) fn scoped_source_snapshot_for_filters(
    root: &Path,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let registration = CodeRepositoryRegistration {
        repository_id: "repo".to_owned(),
        alias: "alias".to_owned(),
        root_path: root.display().to_string(),
        path_filters: path_filters.to_vec(),
        language_filters: language_filters.to_vec(),
    };
    let selector = CodeRepositorySelector {
        repository: "alias".to_owned(),
        ref_selector: ref_selector.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };

    scoped_source_snapshot_inner(&registration, &selector, root, ref_selector, true)
}

pub(in crate::code) fn scoped_source_snapshot_for_registration(
    registration: &CodeRepositoryRegistration,
    ref_selector: &str,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let selector = CodeRepositorySelector {
        repository: registration.alias.clone(),
        ref_selector: ref_selector.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };

    scoped_source_snapshot(registration, &selector, &root, ref_selector)
}

pub(in crate::code) fn scoped_source_snapshot_for_registration_filters(
    registration: &CodeRepositoryRegistration,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let selector = CodeRepositorySelector {
        repository: registration.alias.clone(),
        ref_selector: ref_selector.to_owned(),
        path_filters: path_filters.to_vec(),
        language_filters: language_filters.to_vec(),
    };

    scoped_source_snapshot(registration, &selector, &root, ref_selector)
}

fn scoped_source_snapshot_inner(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    ref_selector: &str,
    allow_filesystem_ref: bool,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let snapshot =
        source_snapshot_for_scope(root, ref_selector, filesystem_policy, allow_filesystem_ref)?;
    let source_layout = discover_source_layout(&snapshot.entries);
    let path_filters = effective_index_path_filters(registration, selector, &source_layout);
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| {
            selection_exclusion_reason_for_source(
                &entry.path,
                registration,
                selector,
                &source_layout,
                snapshot.kind,
            )
            .is_none()
        })
        .collect::<Vec<_>>();
    let (resolved_commit_sha, tree_hash, content_hashes) = if snapshot.kind.is_filesystem() {
        scoped_filesystem_tree_hash(&snapshot.root, &entries, ref_selector)?
    } else {
        (
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            BTreeMap::new(),
        )
    };

    Ok(ScopedSourceSnapshot {
        kind: snapshot.kind,
        root: snapshot.root,
        resolved_commit_sha,
        tree_hash,
        entries,
        content_hashes,
        path_filters,
        language_filters,
    })
}

pub(super) fn source_snapshot_for_scope(
    root: &Path,
    ref_selector: &str,
    filesystem_policy: FileSystemScanPolicy,
    allow_filesystem_ref: bool,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(ref_selector) && allow_filesystem_ref {
        return filesystem_source_snapshot(root, filesystem_policy);
    }

    source_snapshot(root, ref_selector, filesystem_policy)
}

pub(in crate::code) fn filesystem_policy_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> FileSystemScanPolicy {
    let filters = intersect_path_filters(&registration.path_filters, &selector.path_filters);
    let policy = FileSystemScanPolicy::from_path_and_language_filters(
        filters.as_deref().unwrap_or(&[]),
        &registration.language_filters,
        &selector.language_filters,
    );
    if filters.is_none() {
        policy.with_denied_path_scope()
    } else {
        policy
    }
}

pub(super) fn registration_allows_filesystem_ref(
    registration: &CodeRepositoryRegistration,
    root: &Path,
    ref_selector: &str,
) -> Result<bool, CodeIndexError> {
    if !source_commit_is_filesystem(ref_selector) {
        return Ok(false);
    }
    if registration.repository_id == filesystem_registration_identity(root)? {
        return Ok(true);
    }

    Ok(source_kind(root)?.is_filesystem())
}

pub(super) fn scoped_filesystem_tree_hash(
    root: &Path,
    entries: &[GitTreeEntry],
    ref_selector: &str,
) -> Result<(String, String, BTreeMap<String, String>), CodeIndexError> {
    let paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let content_hashes = filesystem_content_hashes_for_paths(root, &paths)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&content_hashes);
    if source_commit_is_filesystem(ref_selector) && ref_selector != tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {ref_selector} no longer matches live indexed scope {tree_hash}"
        )));
    }

    Ok((tree_hash.clone(), tree_hash, content_hashes))
}

#[cfg(test)]
#[path = "scoped_snapshot_tests.rs"]
mod tests;
