use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::{collections::BTreeMap, sync::Mutex};

use super::{
    scope::TrackedEntryScope,
    submodule_repository::{
        submodule_git_dir, submodule_git_dir_from_git_dir, submodule_worktree_root,
    },
};
use crate::code::{
    CodeIndexError,
    source::{
        change_status::split_nul,
        git::{git_bytes, git_bytes_slice, git_dir_bytes},
    },
};

const MAX_SUBMODULE_EXPANSION_DEPTH: usize = 8;

#[cfg(test)]
static TRACKED_ENTRIES_OBSERVER: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
pub(crate) fn reset_tracked_entries_call_count_for_root(root: PathBuf) {
    TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock")
        .insert(root, 0);
}

#[cfg(test)]
pub(crate) fn tracked_entries_call_count_for_root(root: &Path) -> usize {
    TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock")
        .get(root)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
pub(in crate::code) fn tracked_entries(
    root: &Path,
    commit: &str,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    tracked_entries_with_scope(root, commit, &TrackedEntryScope::all())
}

pub(in crate::code) fn tracked_entries_with_scope(
    root: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    Ok(tracked_entries_state_with_scope(root, commit, scope)?.entries)
}

pub(in crate::code) fn tracked_entries_state_with_scope(
    root: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let mut visited = BTreeSet::new();
    tracked_entries_inner(root, commit, "", 0, &mut visited, scope)
}

#[derive(Debug, Clone, Default)]
pub(in crate::code) struct GitTrackedEntries {
    pub(in crate::code) entries: Vec<GitTreeEntry>,
    pub(in crate::code) submodule_states: Vec<String>,
}

fn record_tracked_entries_call(_root: &Path) {
    #[cfg(test)]
    let root = _root;
    #[cfg(test)]
    if let Some(count) = TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock")
        .get_mut(root)
    {
        *count += 1;
    }
}

fn tracked_entries_inner(
    root: &Path,
    commit: &str,
    prefix: &str,
    depth: usize,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    record_tracked_entries_call(root);
    let root_key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let visit_key = (root_key, commit.to_owned());
    if !visited.insert(visit_key.clone()) {
        return Ok(GitTrackedEntries::default());
    }
    let bytes = match tracked_entries_ls_tree_bytes(root, commit, prefix, scope) {
        Ok(bytes) => bytes,
        Err(error) => {
            visited.remove(&visit_key);
            return Err(error);
        }
    };
    let mut state = GitTrackedEntries::default();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        match fields.get(1).copied() {
            Some("blob") if scope.allows_entry(prefix, path) => {
                push_blob_entry(prefix, path, &fields, &mut state.entries);
            }
            Some("commit")
                if depth < MAX_SUBMODULE_EXPANSION_DEPTH
                    && scope.allows_submodule_expansion(&format!("{prefix}{path}")) =>
            {
                let Some(submodule_commit) = fields.get(2) else {
                    continue;
                };
                let next_prefix = format!("{prefix}{path}/");
                match tracked_submodule_entries(
                    TrackedSubmoduleRequest {
                        root,
                        parent_commit: commit,
                        path,
                        submodule_commit,
                        prefix: &next_prefix,
                        depth: depth + 1,
                    },
                    visited,
                    scope,
                ) {
                    Ok(mut submodule_state) => {
                        state
                            .submodule_states
                            .push(format!("expanded\0{prefix}{path}\0{submodule_commit}"));
                        state.entries.append(&mut submodule_state.entries);
                        state
                            .submodule_states
                            .append(&mut submodule_state.submodule_states);
                    }
                    Err(_) => {
                        state
                            .submodule_states
                            .push(format!("unavailable\0{prefix}{path}\0{submodule_commit}"));
                    }
                }
            }
            _ => {}
        }
    }

    visited.remove(&visit_key);

    Ok(state)
}

struct TrackedSubmoduleRequest<'a> {
    root: &'a Path,
    parent_commit: &'a str,
    path: &'a str,
    submodule_commit: &'a str,
    prefix: &'a str,
    depth: usize,
}

fn tracked_submodule_entries(
    request: TrackedSubmoduleRequest<'_>,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    if let Ok(submodule_root) = submodule_worktree_root(request.root, request.path) {
        match tracked_entries_inner(
            &submodule_root,
            request.submodule_commit,
            request.prefix,
            request.depth,
            visited,
            scope,
        ) {
            Ok(state) => return Ok(state),
            Err(error) if tracked_entries_commit_lookup_failed(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let git_dir = submodule_git_dir(
        request.root,
        request.path,
        Some(request.parent_commit),
        Some(request.submodule_commit),
    )?;
    tracked_entries_from_git_dir_inner(
        &git_dir,
        request.submodule_commit,
        request.prefix,
        request.depth,
        visited,
        scope,
    )
}

fn tracked_entries_commit_lookup_failed(error: &CodeIndexError) -> bool {
    matches!(error, CodeIndexError::Git { args, .. } if args.iter().any(|arg| arg == "ls-tree"))
}

fn push_blob_entry(prefix: &str, path: &str, fields: &[&str], entries: &mut Vec<GitTreeEntry>) {
    let byte_count = fields
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    entries.push(GitTreeEntry {
        path: format!("{prefix}{path}"),
        byte_count,
    });
}

pub(in crate::code) fn tracked_entries_from_git_dir_with_scope(
    git_dir: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    let mut visited = BTreeSet::new();
    Ok(tracked_entries_from_git_dir_inner(git_dir, commit, "", 0, &mut visited, scope)?.entries)
}

fn tracked_entries_from_git_dir_inner(
    git_dir: &Path,
    commit: &str,
    prefix: &str,
    depth: usize,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let git_dir_key = git_dir
        .canonicalize()
        .unwrap_or_else(|_| git_dir.to_path_buf());
    let visit_key = (git_dir_key, commit.to_owned());
    if !visited.insert(visit_key.clone()) {
        return Ok(GitTrackedEntries::default());
    }
    let bytes = match tracked_entries_git_dir_ls_tree_bytes(git_dir, commit, prefix, scope) {
        Ok(bytes) => bytes,
        Err(error) => {
            visited.remove(&visit_key);
            return Err(error);
        }
    };
    let mut state = GitTrackedEntries::default();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        match fields.get(1).copied() {
            Some("blob") if scope.allows_entry(prefix, path) => {
                push_blob_entry(prefix, path, &fields, &mut state.entries);
            }
            Some("commit")
                if depth < MAX_SUBMODULE_EXPANSION_DEPTH
                    && scope.allows_submodule_expansion(&format!("{prefix}{path}")) =>
            {
                let Some(submodule_commit) = fields.get(2) else {
                    continue;
                };
                let next_prefix = format!("{prefix}{path}/");
                match tracked_git_dir_submodule_entries(
                    GitDirSubmoduleRequest {
                        parent_git_dir: git_dir,
                        parent_commit: commit,
                        path,
                        submodule_commit,
                        prefix: &next_prefix,
                        depth: depth + 1,
                    },
                    visited,
                    scope,
                ) {
                    Ok(mut submodule_state) => {
                        state
                            .submodule_states
                            .push(format!("expanded\0{prefix}{path}\0{submodule_commit}"));
                        state.entries.append(&mut submodule_state.entries);
                        state
                            .submodule_states
                            .append(&mut submodule_state.submodule_states);
                    }
                    Err(_) => {
                        state
                            .submodule_states
                            .push(format!("unavailable\0{prefix}{path}\0{submodule_commit}"));
                    }
                }
            }
            _ => {}
        }
    }

    visited.remove(&visit_key);

    Ok(state)
}

fn tracked_entries_ls_tree_bytes(
    root: &Path,
    commit: &str,
    prefix: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<u8>, CodeIndexError> {
    if scope.excludes_all_entries() {
        return Ok(Vec::new());
    }
    let Some(pathspecs) = scope.entry_pathspecs(prefix) else {
        return git_bytes(root, ["ls-tree", "-r", "-l", "-z", commit]);
    };
    let mut args = vec!["ls-tree", "-r", "-l", "-z", commit, "--"];
    args.extend(pathspecs.paths.iter().map(String::as_str));
    let mut bytes = git_bytes_slice(root, &args)?;
    for candidate in &pathspecs.gitlink_candidates {
        let mut candidate_bytes =
            git_bytes_slice(root, &["ls-tree", "-l", "-z", commit, "--", candidate])?;
        bytes.append(&mut candidate_bytes);
    }

    Ok(bytes)
}

fn tracked_entries_git_dir_ls_tree_bytes(
    git_dir: &Path,
    commit: &str,
    prefix: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<u8>, CodeIndexError> {
    if scope.excludes_all_entries() {
        return Ok(Vec::new());
    }
    let Some(pathspecs) = scope.entry_pathspecs(prefix) else {
        return git_dir_bytes(git_dir, &["ls-tree", "-r", "-l", "-z", commit]);
    };
    let mut args = vec!["ls-tree", "-r", "-l", "-z", commit, "--"];
    args.extend(pathspecs.paths.iter().map(String::as_str));
    let mut bytes = git_dir_bytes(git_dir, &args)?;
    for candidate in &pathspecs.gitlink_candidates {
        let mut candidate_bytes =
            git_dir_bytes(git_dir, &["ls-tree", "-l", "-z", commit, "--", candidate])?;
        bytes.append(&mut candidate_bytes);
    }

    Ok(bytes)
}

struct GitDirSubmoduleRequest<'a> {
    parent_git_dir: &'a Path,
    parent_commit: &'a str,
    path: &'a str,
    submodule_commit: &'a str,
    prefix: &'a str,
    depth: usize,
}

fn tracked_git_dir_submodule_entries(
    request: GitDirSubmoduleRequest<'_>,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let git_dir = submodule_git_dir_from_git_dir(
        request.parent_git_dir,
        request.path,
        Some(request.parent_commit),
        Some(request.submodule_commit),
    )?;
    tracked_entries_from_git_dir_inner(
        &git_dir,
        request.submodule_commit,
        request.prefix,
        request.depth,
        visited,
        scope,
    )
}

#[derive(Debug, Clone)]
pub(in crate::code) struct GitTreeEntry {
    pub(in crate::code) path: String,
    pub(in crate::code) byte_count: usize,
}

#[cfg(test)]
#[path = "tracked_entries_tests.rs"]
mod tests;
