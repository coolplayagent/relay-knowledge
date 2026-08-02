use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::{
    commands::{
        git_root_has_commit, git_tree_entry, git_tree_entry_from_git_dir,
        submodule_worktree_root_for_commit,
    },
    entries::submodule_path_entries_with_scope,
    paths::{
        SubmoduleChangedPathSets, SubmodulePathEntry, ensure_gitlink_expansion_budget,
        submodule_expansion_is_unavailable,
    },
    selector::GitlinkPathSelector,
};
use crate::code::{
    CodeIndexError,
    source::{
        changes::{
            GitChange, TrackedEntryScope, diff_changes, parse_name_status_z, submodule_git_dir,
            submodule_git_dir_from_git_dir, tracked_entries_from_git_dir_with_scope,
        },
        git::{git_bytes, git_dir_bytes},
    },
};

const MAX_NESTED_GITLINK_DIFF_DEPTH: usize = 8;

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct SubmoduleDiffRequest<'a> {
    pub(super) root: &'a Path,
    pub(super) path: &'a str,
    pub(super) git_dir: Option<&'a Path>,
    pub(super) base_parent_commit: &'a str,
    pub(super) head_parent_commit: &'a str,
    pub(super) base_gitlink: &'a str,
    pub(super) head_gitlink: &'a str,
    pub(super) max_paths: usize,
}

pub(super) fn changed_submodule_path_sets(
    request: SubmoduleDiffRequest<'_>,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<SubmoduleChangedPathSets>, CodeIndexError> {
    changed_submodule_path_sets_inner(request, 0, selector)
}

fn changed_submodule_path_sets_inner(
    request: SubmoduleDiffRequest<'_>,
    depth: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<SubmoduleChangedPathSets>, CodeIndexError> {
    if request.base_gitlink == request.head_gitlink {
        return Ok(Some(SubmoduleChangedPathSets {
            base_paths: BTreeSet::new(),
            head_paths: BTreeSet::new(),
        }));
    }
    let changes = match diff_submodule_changes(
        request.root,
        request.path,
        request.git_dir,
        request.base_parent_commit,
        request.base_gitlink,
        request.head_gitlink,
    ) {
        Ok(changes) => changes,
        Err(_) => return Ok(None),
    };
    let resolved_git_dir = if request.git_dir.is_none() {
        current_submodule_git_dir(
            request.root,
            request.path,
            request.base_parent_commit,
            request.base_gitlink,
            request.head_gitlink,
        )?
    } else {
        None
    };
    let current_git_dir = request.git_dir.or(resolved_git_dir.as_deref());
    let mut base_paths = BTreeSet::new();
    let mut head_paths = BTreeSet::new();
    let parent_path = request.path;
    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if !append_side_nested_gitlink_paths(
                    SideNestedGitlinkRequest {
                        root: request.root,
                        parent_path,
                        parent_git_dir: current_git_dir,
                        parent_commit: request.base_parent_commit,
                        parent_gitlink: request.base_gitlink,
                        child_path: &path,
                        max_paths: request.max_paths,
                    },
                    &mut base_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut base_paths, parent_path, &path, selector);
                }
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                if !append_changed_nested_gitlink_paths(
                    NestedGitlinkChange {
                        root: request.root,
                        parent_path,
                        parent_git_dir: current_git_dir,
                        base_parent_commit: request.base_parent_commit,
                        head_parent_commit: request.head_parent_commit,
                        base_gitlink: request.base_gitlink,
                        head_gitlink: request.head_gitlink,
                        child_path: &path,
                        max_paths: request.max_paths,
                        depth,
                    },
                    &mut base_paths,
                    &mut head_paths,
                    selector,
                )? {
                    let parent_child_path = parent_submodule_path(parent_path, &path);
                    if selector.includes(&parent_child_path) {
                        if submodule_blob_exists(
                            request.root,
                            parent_path,
                            current_git_dir,
                            request.base_parent_commit,
                            request.base_gitlink,
                            &path,
                        )? {
                            base_paths.insert(parent_child_path.clone());
                        }
                        head_paths.insert(parent_child_path);
                    }
                }
            }
            GitChange::Renamed { old_path, new_path } => {
                if !append_side_nested_gitlink_paths(
                    SideNestedGitlinkRequest {
                        root: request.root,
                        parent_path,
                        parent_git_dir: current_git_dir,
                        parent_commit: request.base_parent_commit,
                        parent_gitlink: request.base_gitlink,
                        child_path: &old_path,
                        max_paths: request.max_paths,
                    },
                    &mut base_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut base_paths, parent_path, &old_path, selector);
                }
                if !append_side_nested_gitlink_paths(
                    SideNestedGitlinkRequest {
                        root: request.root,
                        parent_path,
                        parent_git_dir: current_git_dir,
                        parent_commit: request.head_parent_commit,
                        parent_gitlink: request.head_gitlink,
                        child_path: &new_path,
                        max_paths: request.max_paths,
                    },
                    &mut head_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut head_paths, parent_path, &new_path, selector);
                }
            }
            GitChange::Copied { new_path, .. } => {
                if !append_side_nested_gitlink_paths(
                    SideNestedGitlinkRequest {
                        root: request.root,
                        parent_path,
                        parent_git_dir: current_git_dir,
                        parent_commit: request.head_parent_commit,
                        parent_gitlink: request.head_gitlink,
                        child_path: &new_path,
                        max_paths: request.max_paths,
                    },
                    &mut head_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut head_paths, parent_path, &new_path, selector);
                }
            }
        }
        ensure_gitlink_expansion_budget(
            request.path,
            base_paths.len().saturating_add(head_paths.len()),
            request.max_paths,
        )?;
    }

    Ok(Some(SubmoduleChangedPathSets {
        base_paths,
        head_paths,
    }))
}

fn insert_selected_parent_path(
    paths: &mut BTreeSet<String>,
    parent_path: &str,
    child_path: &str,
    selector: &GitlinkPathSelector<'_>,
) {
    insert_selected_path(
        paths,
        &parent_submodule_path(parent_path, child_path),
        selector,
    );
}

fn insert_selected_path(
    paths: &mut BTreeSet<String>,
    path: &str,
    selector: &GitlinkPathSelector<'_>,
) {
    if selector.includes(path) {
        paths.insert(path.to_owned());
    }
}

struct NestedGitlinkChange<'a> {
    root: &'a Path,
    parent_path: &'a str,
    parent_git_dir: Option<&'a Path>,
    base_parent_commit: &'a str,
    head_parent_commit: &'a str,
    base_gitlink: &'a str,
    head_gitlink: &'a str,
    child_path: &'a str,
    max_paths: usize,
    depth: usize,
}

fn append_changed_nested_gitlink_paths(
    change: NestedGitlinkChange<'_>,
    base_paths: &mut BTreeSet<String>,
    head_paths: &mut BTreeSet<String>,
    selector: &GitlinkPathSelector<'_>,
) -> Result<bool, CodeIndexError> {
    let base_nested = submodule_gitlink_commit(
        change.root,
        change.parent_path,
        change.parent_git_dir,
        change.base_parent_commit,
        change.base_gitlink,
        change.child_path,
    )?;
    let head_nested = submodule_gitlink_commit(
        change.root,
        change.parent_path,
        change.parent_git_dir,
        change.head_parent_commit,
        change.head_gitlink,
        change.child_path,
    )?;
    if base_nested.is_none() && head_nested.is_none() {
        return Ok(false);
    }
    let nested_parent_path = parent_submodule_path(change.parent_path, change.child_path);
    if !selector.overlaps(&nested_parent_path) {
        return Ok(true);
    }
    if let (Some(base_commit), Some(head_commit)) = (&base_nested, &head_nested) {
        if base_commit == head_commit {
            return Ok(true);
        }
        let nested_git_dir = nested_submodule_git_dir(
            change.parent_git_dir,
            change.child_path,
            change.base_gitlink,
            base_commit,
        )?
        .or(nested_submodule_git_dir(
            change.parent_git_dir,
            change.child_path,
            change.head_gitlink,
            head_commit,
        )?);
        if change.depth < MAX_NESTED_GITLINK_DIFF_DEPTH
            && let Some(changed_paths) = changed_submodule_path_sets_inner(
                SubmoduleDiffRequest {
                    root: change.root,
                    path: &nested_parent_path,
                    git_dir: nested_git_dir.as_deref(),
                    base_parent_commit: change.base_gitlink,
                    head_parent_commit: change.head_gitlink,
                    base_gitlink: base_commit,
                    head_gitlink: head_commit,
                    max_paths: change.max_paths,
                },
                change.depth + 1,
                selector,
            )?
        {
            base_paths.extend(changed_paths.base_paths);
            head_paths.extend(changed_paths.head_paths);
            return Ok(true);
        }
    }

    match base_nested {
        Some(commit) => append_bounded_submodule_entry_paths(
            SubmoduleEntryExpansion {
                root: change.root,
                path: &nested_parent_path,
                git_dir: nested_submodule_git_dir(
                    change.parent_git_dir,
                    change.child_path,
                    change.base_gitlink,
                    &commit,
                )?,
                parent_commit: change.base_gitlink,
                commit: &commit,
                max_paths: change.max_paths,
            },
            base_paths,
            selector,
        )?,
        None if submodule_blob_exists(
            change.root,
            change.parent_path,
            change.parent_git_dir,
            change.base_parent_commit,
            change.base_gitlink,
            change.child_path,
        )? =>
        {
            insert_selected_path(
                base_paths,
                &parent_submodule_path(change.parent_path, change.child_path),
                selector,
            );
        }
        None => {}
    }
    match head_nested {
        Some(commit) => append_bounded_submodule_entry_paths(
            SubmoduleEntryExpansion {
                root: change.root,
                path: &nested_parent_path,
                git_dir: nested_submodule_git_dir(
                    change.parent_git_dir,
                    change.child_path,
                    change.head_gitlink,
                    &commit,
                )?,
                parent_commit: change.head_gitlink,
                commit: &commit,
                max_paths: change.max_paths,
            },
            head_paths,
            selector,
        )?,
        None => {
            insert_selected_path(
                head_paths,
                &parent_submodule_path(change.parent_path, change.child_path),
                selector,
            );
        }
    }

    Ok(true)
}

struct SideNestedGitlinkRequest<'a> {
    root: &'a Path,
    parent_path: &'a str,
    parent_git_dir: Option<&'a Path>,
    parent_commit: &'a str,
    parent_gitlink: &'a str,
    child_path: &'a str,
    max_paths: usize,
}

fn append_side_nested_gitlink_paths(
    request: SideNestedGitlinkRequest<'_>,
    paths: &mut BTreeSet<String>,
    selector: &GitlinkPathSelector<'_>,
) -> Result<bool, CodeIndexError> {
    let Some(nested_commit) = submodule_gitlink_commit(
        request.root,
        request.parent_path,
        request.parent_git_dir,
        request.parent_commit,
        request.parent_gitlink,
        request.child_path,
    )?
    else {
        return Ok(false);
    };
    let nested_parent_path = parent_submodule_path(request.parent_path, request.child_path);
    if !selector.overlaps(&nested_parent_path) {
        return Ok(true);
    }
    append_bounded_submodule_entry_paths(
        SubmoduleEntryExpansion {
            root: request.root,
            path: &nested_parent_path,
            git_dir: nested_submodule_git_dir(
                request.parent_git_dir,
                request.child_path,
                request.parent_gitlink,
                &nested_commit,
            )?,
            parent_commit: request.parent_gitlink,
            commit: &nested_commit,
            max_paths: request.max_paths,
        },
        paths,
        selector,
    )?;

    Ok(true)
}

struct SubmoduleEntryExpansion<'a> {
    root: &'a Path,
    path: &'a str,
    git_dir: Option<PathBuf>,
    parent_commit: &'a str,
    commit: &'a str,
    max_paths: usize,
}

fn append_bounded_submodule_entry_paths(
    request: SubmoduleEntryExpansion<'_>,
    paths: &mut BTreeSet<String>,
    selector: &GitlinkPathSelector<'_>,
) -> Result<(), CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(
        request.root,
        request.path,
        request.git_dir.as_deref(),
        Some(request.parent_commit),
        request.commit,
        selector,
    )?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<Vec<_>>();
    ensure_gitlink_expansion_budget(request.path, selected.len(), request.max_paths)?;
    paths.extend(selected);

    Ok(())
}

fn submodule_gitlink_commit(
    root: &Path,
    path: &str,
    git_dir: Option<&Path>,
    parent_commit: &str,
    commit: &str,
    child_path: &str,
) -> Result<Option<String>, CodeIndexError> {
    if let Some(git_dir) = git_dir {
        return Ok(git_tree_entry_from_git_dir(git_dir, commit, child_path)?
            .filter(|entry| entry.kind == "commit")
            .map(|entry| entry.object));
    }
    if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
        Ok(git_tree_entry(&submodule_root, commit, child_path)?
            .filter(|entry| entry.kind == "commit")
            .map(|entry| entry.object))
    } else {
        let git_dir = submodule_git_dir(root, path, Some(parent_commit), Some(commit))?;
        Ok(git_tree_entry_from_git_dir(&git_dir, commit, child_path)?
            .filter(|entry| entry.kind == "commit")
            .map(|entry| entry.object))
    }
}

fn current_submodule_git_dir(
    root: &Path,
    path: &str,
    parent_commit: &str,
    base_gitlink: &str,
    _head_gitlink: &str,
) -> Result<Option<PathBuf>, CodeIndexError> {
    match submodule_git_dir(root, path, Some(parent_commit), Some(base_gitlink)) {
        Ok(git_dir) => Ok(Some(git_dir)),
        Err(error) if submodule_expansion_is_unavailable(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn nested_submodule_git_dir(
    parent_git_dir: Option<&Path>,
    path: &str,
    parent_commit: &str,
    commit: &str,
) -> Result<Option<PathBuf>, CodeIndexError> {
    let Some(parent_git_dir) = parent_git_dir else {
        return Ok(None);
    };
    match submodule_git_dir_from_git_dir(parent_git_dir, path, Some(parent_commit), Some(commit)) {
        Ok(git_dir) => Ok(Some(git_dir)),
        Err(error) if submodule_expansion_is_unavailable(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn diff_submodule_changes(
    root: &Path,
    path: &str,
    git_dir: Option<&Path>,
    base_parent_commit: &str,
    base_gitlink: &str,
    head_gitlink: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    if let Some(git_dir) = git_dir {
        return diff_changes_from_git_dir(git_dir, base_gitlink, head_gitlink);
    }
    if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, base_gitlink)
        && git_root_has_commit(&submodule_root, head_gitlink)
    {
        diff_changes(&submodule_root, base_gitlink, head_gitlink)
    } else {
        diff_changes_from_git_dir(
            &submodule_git_dir(root, path, Some(base_parent_commit), Some(base_gitlink))?,
            base_gitlink,
            head_gitlink,
        )
    }
}

fn submodule_blob_exists(
    root: &Path,
    path: &str,
    git_dir: Option<&Path>,
    parent_commit: &str,
    commit: &str,
    child_path: &str,
) -> Result<bool, CodeIndexError> {
    if let Some(git_dir) = git_dir {
        return Ok(git_dir_bytes(
            git_dir,
            &["cat-file", "-e", &format!("{commit}:{child_path}")],
        )
        .is_ok());
    }
    if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
        Ok(git_bytes(
            &submodule_root,
            ["cat-file", "-e", &format!("{commit}:{child_path}")],
        )
        .is_ok())
    } else {
        Ok(git_dir_bytes(
            &submodule_git_dir(root, path, Some(parent_commit), Some(commit))?,
            &["cat-file", "-e", &format!("{commit}:{child_path}")],
        )
        .is_ok())
    }
}

fn diff_changes_from_git_dir(
    git_dir: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    let bytes = git_dir_bytes(
        git_dir,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            "--end-of-options",
            base_ref,
            head_ref,
            "--",
        ],
    )?;

    parse_name_status_z(&bytes)
}

fn parent_submodule_path(parent_path: &str, child_path: &str) -> String {
    format!("{}/{}", parent_path.trim_end_matches('/'), child_path)
}

pub(super) fn bounded_submodule_parent_paths(
    root: &Path,
    path: &str,
    git_dir: Option<&Path>,
    parent_commit: &str,
    commit: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(
        root,
        path,
        git_dir,
        Some(parent_commit),
        commit,
        selector,
    )?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<BTreeSet<_>>();
    ensure_gitlink_expansion_budget(path, selected.len(), max_paths)?;

    Ok(selected)
}

fn submodule_path_entries_for_expansion(
    root: &Path,
    path: &str,
    git_dir: Option<&Path>,
    parent_commit: Option<&str>,
    commit: &str,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    let Some(child_filters) = selector.child_filters(path) else {
        return Ok(Vec::new());
    };
    let scope = TrackedEntryScope::from_entry_path_filters(child_filters.iter());
    let entries = match git_dir {
        Some(git_dir) => {
            submodule_path_entries_from_git_dir_with_scope(git_dir, path, commit, &scope)
        }
        None => submodule_path_entries_with_scope(root, path, parent_commit, commit, &scope),
    };
    match entries {
        Ok(entries) => Ok(entries),
        Err(error) if submodule_expansion_is_unavailable(&error) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn submodule_path_entries_from_git_dir_with_scope(
    git_dir: &Path,
    path: &str,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let entries = tracked_entries_from_git_dir_with_scope(git_dir, commit, scope)?;

    Ok(entries
        .into_iter()
        .map(|entry| SubmodulePathEntry {
            parent_path: format!("{prefix}{}", entry.path),
            child_path: entry.path,
        })
        .collect())
}
