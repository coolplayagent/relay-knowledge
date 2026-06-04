use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::{
    CodeIndexError,
    changes::{
        GitChange, TrackedEntryScope, diff_changes, git_dir_bytes, parse_name_status_z,
        submodule_git_dir, submodule_worktree_root, tracked_entries_from_git_dir_with_scope,
        tracked_entries_with_scope,
    },
    git::git_bytes,
};

pub(super) use super::source_gitlink_paths::{
    GitlinkPathExpansion, SubmoduleChangedPathSets, SubmodulePathEntry,
};
pub(super) use super::source_gitlink_selector::GitlinkPathSelector;
pub(super) use super::source_gitlink_target::{
    git_blob_bytes_with_submodules, git_dir_blob_bytes_with_submodules, submodule_blob_size,
    submodule_bytes,
};

const MAX_NESTED_GITLINK_DIFF_DEPTH: usize = 8;

pub(super) struct GitlinkImpactExpander<'a> {
    root: &'a Path,
    base_commit: String,
    head_commit: String,
    max_paths: usize,
}

impl<'a> GitlinkImpactExpander<'a> {
    pub(super) fn new(
        root: &'a Path,
        base_commit: String,
        head_commit: String,
        max_paths: usize,
    ) -> Self {
        Self {
            root,
            base_commit,
            head_commit,
            max_paths,
        }
    }

    pub(super) fn expanded_paths(
        &mut self,
        path: &str,
        include_base: bool,
        include_head: bool,
        selector: &GitlinkPathSelector<'_>,
    ) -> Result<Option<Vec<String>>, CodeIndexError> {
        let base_gitlink = include_base
            .then(|| gitlink_commit_at_tree(self.root, &self.base_commit, path))
            .transpose()?
            .flatten();
        let head_gitlink = include_head
            .then(|| gitlink_commit_at_tree(self.root, &self.head_commit, path))
            .transpose()?
            .flatten();
        if base_gitlink.is_none() && head_gitlink.is_none() {
            return Ok(None);
        }
        if base_gitlink.is_some()
            && head_gitlink.is_some()
            && let Some(paths) = changed_submodule_paths_for_parent_commits(
                self.root,
                path,
                &self.base_commit,
                &self.head_commit,
                self.max_paths,
                selector,
            )?
        {
            return Ok(Some(paths.into_iter().collect()));
        }

        let max_paths = self.max_paths;
        let base_paths = match &base_gitlink {
            Some(commit) => bounded_submodule_parent_paths(
                self.root,
                path,
                &self.base_commit,
                commit,
                max_paths,
                selector,
            )?,
            None => BTreeSet::new(),
        };
        let head_paths = match &head_gitlink {
            Some(commit) => bounded_submodule_parent_paths(
                self.root,
                path,
                &self.head_commit,
                commit,
                max_paths,
                selector,
            )?,
            None => BTreeSet::new(),
        };
        let mut paths = base_paths.union(&head_paths).cloned().collect::<Vec<_>>();
        if include_base && base_gitlink.is_none() && selector.includes(path) {
            paths.push(path.to_owned());
        }
        if include_head && head_gitlink.is_none() && selector.includes(path) {
            paths.push(path.to_owned());
        }
        paths.sort();
        paths.dedup();
        ensure_gitlink_expansion_budget(path, paths.len(), max_paths)?;

        Ok(Some(paths))
    }
}

pub(super) fn changed_gitlink_path_expansion(
    root: &Path,
    path: &str,
    base_parent_commit: &str,
    head_parent_commit: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<GitlinkPathExpansion>, CodeIndexError> {
    let base_gitlink = gitlink_commit_at_tree(root, base_parent_commit, path)?;
    let head_gitlink = gitlink_commit_at_tree(root, head_parent_commit, path)?;
    if base_gitlink.is_none() && head_gitlink.is_none() {
        return Ok(None);
    }

    if let (Some(base_gitlink), Some(head_gitlink)) = (&base_gitlink, &head_gitlink) {
        let Some(changed_paths) = changed_submodule_path_sets(
            SubmoduleDiffRequest {
                root,
                path,
                base_parent_commit,
                head_parent_commit,
                base_gitlink,
                head_gitlink,
                max_paths,
            },
            selector,
        )?
        else {
            let base_paths = bounded_submodule_parent_paths(
                root,
                path,
                base_parent_commit,
                base_gitlink,
                max_paths,
                selector,
            )?;
            let head_paths = bounded_submodule_parent_paths(
                root,
                path,
                head_parent_commit,
                head_gitlink,
                max_paths,
                selector,
            )?;
            ensure_gitlink_expansion_budget(
                path,
                base_paths.len().saturating_add(head_paths.len()),
                max_paths,
            )?;
            return Ok(Some(GitlinkPathExpansion {
                base_is_gitlink: true,
                head_is_gitlink: true,
                base_paths,
                head_paths,
            }));
        };
        return Ok(Some(GitlinkPathExpansion {
            base_is_gitlink: true,
            head_is_gitlink: true,
            base_paths: changed_paths.base_paths,
            head_paths: changed_paths.head_paths,
        }));
    }

    let base_paths = match &base_gitlink {
        Some(commit) => bounded_submodule_parent_paths(
            root,
            path,
            base_parent_commit,
            commit,
            max_paths,
            selector,
        )?,
        None => BTreeSet::new(),
    };
    let head_paths = match &head_gitlink {
        Some(commit) => bounded_submodule_parent_paths(
            root,
            path,
            head_parent_commit,
            commit,
            max_paths,
            selector,
        )?,
        None => BTreeSet::new(),
    };

    Ok(Some(GitlinkPathExpansion {
        base_is_gitlink: base_gitlink.is_some(),
        head_is_gitlink: head_gitlink.is_some(),
        base_paths,
        head_paths,
    }))
}

pub(super) fn gitlink_commit_at_tree(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<String>, CodeIndexError> {
    Ok(git_tree_entry(root, commit, path)?
        .filter(|entry| entry.kind == "commit")
        .map(|entry| entry.object))
}

pub(super) fn submodule_path_entries_with_child_filters(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    child_filters: &[String],
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    submodule_path_entries_with_scope(
        root,
        path,
        parent_commit,
        commit,
        &TrackedEntryScope::from_entry_path_filters(child_filters.iter()),
    )
}

fn submodule_path_entries_with_scope(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let entries =
        if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
            tracked_entries_with_scope(&submodule_root, commit, scope)?
        } else {
            tracked_entries_from_git_dir_with_scope(
                &submodule_git_dir(root, path, parent_commit, Some(commit))?,
                commit,
                scope,
            )?
        };

    Ok(entries
        .into_iter()
        .map(|entry| SubmodulePathEntry {
            parent_path: format!("{prefix}{}", entry.path),
            child_path: entry.path,
        })
        .collect())
}

pub(super) fn submodule_entry_bytes(
    root: &Path,
    path: &str,
    commit: &str,
    child_path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
        git_blob_bytes_with_submodules(&submodule_root, commit, child_path)
    } else {
        git_dir_blob_bytes_with_submodules(
            &submodule_git_dir(root, path, None, Some(commit))?,
            commit,
            child_path,
        )
    }
}

pub(super) fn submodule_root(root: &Path, path: &str) -> Result<PathBuf, CodeIndexError> {
    submodule_worktree_root(root, path)
}

fn changed_submodule_paths_for_parent_commits(
    root: &Path,
    path: &str,
    base_parent_commit: &str,
    head_parent_commit: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<BTreeSet<String>>, CodeIndexError> {
    let Some(base_gitlink) = gitlink_commit_at_tree(root, base_parent_commit, path)? else {
        return Ok(None);
    };
    let Some(head_gitlink) = gitlink_commit_at_tree(root, head_parent_commit, path)? else {
        return Ok(None);
    };
    let Some(changed_paths) = changed_submodule_path_sets(
        SubmoduleDiffRequest {
            root,
            path,
            base_parent_commit,
            head_parent_commit,
            base_gitlink: &base_gitlink,
            head_gitlink: &head_gitlink,
            max_paths,
        },
        selector,
    )?
    else {
        let mut paths = bounded_submodule_parent_paths(
            root,
            path,
            base_parent_commit,
            &base_gitlink,
            max_paths,
            selector,
        )?;
        paths.extend(bounded_submodule_parent_paths(
            root,
            path,
            head_parent_commit,
            &head_gitlink,
            max_paths,
            selector,
        )?);
        ensure_gitlink_expansion_budget(path, paths.len(), max_paths)?;
        return Ok(Some(paths));
    };

    Ok(Some(
        changed_paths
            .base_paths
            .union(&changed_paths.head_paths)
            .cloned()
            .collect(),
    ))
}

#[derive(Clone, Copy)]
struct SubmoduleDiffRequest<'a> {
    root: &'a Path,
    path: &'a str,
    base_parent_commit: &'a str,
    head_parent_commit: &'a str,
    base_gitlink: &'a str,
    head_gitlink: &'a str,
    max_paths: usize,
}

fn changed_submodule_path_sets(
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
        request.base_parent_commit,
        request.base_gitlink,
        request.head_gitlink,
    ) {
        Ok(changes) => changes,
        Err(_) => return Ok(None),
    };
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
        change.base_parent_commit,
        change.base_gitlink,
        change.child_path,
    )?;
    let head_nested = submodule_gitlink_commit(
        change.root,
        change.parent_path,
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
        if change.depth < MAX_NESTED_GITLINK_DIFF_DEPTH
            && let Some(changed_paths) = changed_submodule_path_sets_inner(
                SubmoduleDiffRequest {
                    root: change.root,
                    path: &nested_parent_path,
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
            change.root,
            &nested_parent_path,
            change.base_gitlink,
            &commit,
            base_paths,
            change.max_paths,
            selector,
        )?,
        None if submodule_blob_exists(
            change.root,
            change.parent_path,
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
            change.root,
            &nested_parent_path,
            change.head_gitlink,
            &commit,
            head_paths,
            change.max_paths,
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
        request.root,
        &nested_parent_path,
        request.parent_gitlink,
        &nested_commit,
        paths,
        request.max_paths,
        selector,
    )?;

    Ok(true)
}

fn append_bounded_submodule_entry_paths(
    root: &Path,
    path: &str,
    parent_commit: &str,
    commit: &str,
    paths: &mut BTreeSet<String>,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<(), CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(root, path, Some(parent_commit), commit)?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<Vec<_>>();
    ensure_gitlink_expansion_budget(path, selected.len(), max_paths)?;
    paths.extend(selected);

    Ok(())
}

fn submodule_gitlink_commit(
    root: &Path,
    path: &str,
    parent_commit: &str,
    commit: &str,
    child_path: &str,
) -> Result<Option<String>, CodeIndexError> {
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

fn diff_submodule_changes(
    root: &Path,
    path: &str,
    base_parent_commit: &str,
    base_gitlink: &str,
    head_gitlink: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
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
    parent_commit: &str,
    commit: &str,
    child_path: &str,
) -> Result<bool, CodeIndexError> {
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

fn bounded_submodule_parent_paths(
    root: &Path,
    path: &str,
    parent_commit: &str,
    commit: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(root, path, Some(parent_commit), commit)?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<BTreeSet<_>>();
    ensure_gitlink_expansion_budget(path, selected.len(), max_paths)?;

    Ok(selected)
}

fn ensure_gitlink_expansion_budget(
    path: &str,
    expanded_count: usize,
    max_paths: usize,
) -> Result<(), CodeIndexError> {
    if expanded_count <= max_paths {
        return Ok(());
    }

    Err(CodeIndexError::InvalidInput(format!(
        "gitlink path {path} expands to {expanded_count} files; run a full code index so the work is checkpointed and batched"
    )))
}

fn submodule_path_entries_for_expansion(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    match submodule_path_entries_with_scope(
        root,
        path,
        parent_commit,
        commit,
        &TrackedEntryScope::all(),
    ) {
        Ok(entries) => Ok(entries),
        Err(error) if submodule_expansion_is_unavailable(&error) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub(super) fn submodule_expansion_is_unavailable(error: &CodeIndexError) -> bool {
    match error {
        CodeIndexError::InvalidInput(message) => {
            message.contains("submodule git dir") && message.contains("unavailable")
        }
        CodeIndexError::Git { args, .. } => args.iter().any(|arg| arg == "ls-tree"),
        _ => false,
    }
}

pub(super) fn git_root_has_commit(root: &Path, commit: &str) -> bool {
    git_bytes(root, ["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok()
}

fn submodule_worktree_root_for_commit(root: &Path, path: &str, commit: &str) -> Option<PathBuf> {
    submodule_worktree_root(root, path)
        .ok()
        .filter(|submodule_root| git_root_has_commit(submodule_root, commit))
}

#[derive(Debug)]
pub(super) struct GitTreeLookup {
    pub(super) object: String,
    pub(super) kind: String,
}

pub(super) fn git_tree_entry(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<GitTreeLookup>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-z", commit, "--", path])?;
    let Some(record) = bytes
        .split(|byte| *byte == 0)
        .find(|record| !record.is_empty())
    else {
        return Ok(None);
    };
    let record = String::from_utf8_lossy(record);
    let Some((metadata, _)) = record.split_once('\t') else {
        return Ok(None);
    };
    let fields = metadata.split_whitespace().collect::<Vec<_>>();
    let Some(object) = fields.get(2) else {
        return Ok(None);
    };

    Ok(Some(GitTreeLookup {
        object: (*object).to_owned(),
        kind: fields.get(1).copied().unwrap_or_default().to_owned(),
    }))
}

pub(super) fn git_tree_entry_from_git_dir(
    git_dir: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<GitTreeLookup>, CodeIndexError> {
    let bytes = git_dir_bytes(git_dir, &["ls-tree", "-z", commit, "--", path])?;
    let Some(record) = bytes
        .split(|byte| *byte == 0)
        .find(|record| !record.is_empty())
    else {
        return Ok(None);
    };
    let record = String::from_utf8_lossy(record);
    let Some((metadata, _)) = record.split_once('\t') else {
        return Ok(None);
    };
    let fields = metadata.split_whitespace().collect::<Vec<_>>();
    let Some(object) = fields.get(2) else {
        return Ok(None);
    };

    Ok(Some(GitTreeLookup {
        object: (*object).to_owned(),
        kind: fields.get(1).copied().unwrap_or_default().to_owned(),
    }))
}

pub(super) fn safe_relative_path(path: &str) -> bool {
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
