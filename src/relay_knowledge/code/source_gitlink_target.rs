use std::path::Path;

use super::{
    CodeIndexError,
    changes::{
        git_dir_bytes, submodule_git_dir, submodule_git_dir_from_git_dir, submodule_worktree_root,
    },
    git::git_bytes,
    source_gitlink::{git_root_has_commit, git_tree_entry, git_tree_entry_from_git_dir},
    source_gitlink_paths::{GitlinkTarget, GitlinkTargetLocation},
};

pub(super) fn submodule_bytes(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    let target = gitlink_target_for_path(root, commit, path)?;
    match target.location {
        GitlinkTargetLocation::Worktree(root) => {
            git_blob_bytes_with_submodules(&root, &target.commit, &target.path)
        }
        GitlinkTargetLocation::GitDir(git_dir) => {
            git_dir_blob_bytes_with_submodules(&git_dir, &target.commit, &target.path)
        }
    }
}

pub(super) fn submodule_blob_size(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    let target = match gitlink_target_for_path(root, commit, path) {
        Ok(target) => target,
        Err(_) => return Ok(None),
    };
    match target.location {
        GitlinkTargetLocation::Worktree(root) => {
            git_blob_size_with_submodules(&root, &target.commit, &target.path)
        }
        GitlinkTargetLocation::GitDir(git_dir) => {
            git_dir_blob_size_with_submodules(&git_dir, &target.commit, &target.path)
        }
    }
}

pub(super) fn git_blob_bytes_with_submodules(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    match git_bytes(root, ["show", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(bytes),
        Err(error) => submodule_bytes(root, commit, path).map_err(|_| error),
    }
}

pub(super) fn git_dir_blob_bytes_with_submodules(
    git_dir: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    match git_dir_bytes(git_dir, &["show", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(bytes),
        Err(error) => gitlink_target_for_git_dir_path(git_dir, commit, path)
            .and_then(|target| {
                let GitlinkTargetLocation::GitDir(target_git_dir) = target.location else {
                    return Err(CodeIndexError::InvalidInput(
                        "gitdir submodule target unexpectedly resolved to a worktree".to_owned(),
                    ));
                };
                git_dir_blob_bytes_with_submodules(&target_git_dir, &target.commit, &target.path)
            })
            .map_err(|_| error),
    }
}

fn git_blob_size_with_submodules(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    let object = format!("{commit}:{path}");
    match git_bytes(root, ["cat-file", "-s", &object]) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
        Err(_) => submodule_blob_size(root, commit, path),
    }
}

fn git_dir_blob_size_with_submodules(
    git_dir: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    match git_dir_bytes(git_dir, &["cat-file", "-s", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
        Err(_) => match gitlink_target_for_git_dir_path(git_dir, commit, path) {
            Ok(target) => {
                let GitlinkTargetLocation::GitDir(target_git_dir) = target.location else {
                    return Err(CodeIndexError::InvalidInput(
                        "gitdir submodule target unexpectedly resolved to a worktree".to_owned(),
                    ));
                };
                git_dir_blob_size_with_submodules(&target_git_dir, &target.commit, &target.path)
            }
            Err(_) => Ok(None),
        },
    }
}

fn gitlink_target_for_path(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<GitlinkTarget, CodeIndexError> {
    if !super::source_gitlink::safe_relative_path(path) {
        return Err(CodeIndexError::InvalidInput(format!(
            "unsafe repository source path '{path}'"
        )));
    }
    let segments = path.split('/').collect::<Vec<_>>();
    for prefix_len in 1..segments.len() {
        let prefix = segments[..prefix_len].join("/");
        let Some(entry) = git_tree_entry(root, commit, &prefix)? else {
            continue;
        };
        if entry.kind != "commit" {
            continue;
        }
        let location = match submodule_worktree_root(root, &prefix) {
            Ok(submodule_root) if git_root_has_commit(&submodule_root, &entry.object) => {
                GitlinkTargetLocation::Worktree(submodule_root)
            }
            _ => GitlinkTargetLocation::GitDir(submodule_git_dir(
                root,
                &prefix,
                Some(commit),
                Some(&entry.object),
            )?),
        };
        let remaining_path = segments[prefix_len..].join("/");
        return Ok(GitlinkTarget {
            location,
            commit: entry.object,
            path: remaining_path,
        });
    }

    Err(CodeIndexError::InvalidInput(format!(
        "repository source path {path} is not a checked-out submodule path"
    )))
}

fn gitlink_target_for_git_dir_path(
    git_dir: &Path,
    commit: &str,
    path: &str,
) -> Result<GitlinkTarget, CodeIndexError> {
    if !super::source_gitlink::safe_relative_path(path) {
        return Err(CodeIndexError::InvalidInput(format!(
            "unsafe repository source path '{path}'"
        )));
    }
    let segments = path.split('/').collect::<Vec<_>>();
    for prefix_len in 1..segments.len() {
        let prefix = segments[..prefix_len].join("/");
        let Some(entry) = git_tree_entry_from_git_dir(git_dir, commit, &prefix)? else {
            continue;
        };
        if entry.kind != "commit" {
            continue;
        }
        let remaining_path = segments[prefix_len..].join("/");
        return Ok(GitlinkTarget {
            location: GitlinkTargetLocation::GitDir(submodule_git_dir_from_git_dir(
                git_dir,
                &prefix,
                Some(commit),
                Some(&entry.object),
            )?),
            commit: entry.object,
            path: remaining_path,
        });
    }

    Err(CodeIndexError::InvalidInput(format!(
        "repository source path {path} is not a checked-out submodule path"
    )))
}
