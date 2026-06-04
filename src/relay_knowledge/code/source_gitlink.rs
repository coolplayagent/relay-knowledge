use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::{
    CodeIndexError,
    changes::{
        GitChange, GitTreeEntry, TrackedEntryScope, diff_changes, git_dir_bytes,
        parse_name_status_z, submodule_git_dir, submodule_git_dir_from_git_dir,
        submodule_worktree_root, tracked_entries, tracked_entries_from_git_dir_with_scope,
        tracked_entries_with_scope,
    },
    git::git_bytes,
    source_gitlink_paths::bounded_expanded_paths_under_with_selector,
};

pub(super) use super::source_gitlink_paths::bounded_expanded_paths_under;
pub(super) use super::source_gitlink_selector::GitlinkPathSelector;

const MAX_NESTED_GITLINK_DIFF_DEPTH: usize = 8;

#[derive(Debug)]
struct GitlinkTarget {
    location: GitlinkTargetLocation,
    commit: String,
    path: String,
}

#[derive(Debug)]
enum GitlinkTargetLocation {
    Worktree(PathBuf),
    GitDir(PathBuf),
}

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

pub(super) struct GitlinkImpactExpander<'a> {
    root: &'a Path,
    base_commit: String,
    head_commit: String,
    base_entries: Option<Vec<GitTreeEntry>>,
    head_entries: Option<Vec<GitTreeEntry>>,
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
            base_entries: None,
            head_entries: None,
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
        let base_is_gitlink =
            include_base && gitlink_commit_at_tree(self.root, &self.base_commit, path)?.is_some();
        let head_is_gitlink =
            include_head && gitlink_commit_at_tree(self.root, &self.head_commit, path)?.is_some();
        if !base_is_gitlink && !head_is_gitlink {
            return Ok(None);
        }
        if base_is_gitlink
            && head_is_gitlink
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
        let base_paths = if base_is_gitlink {
            let base_entries = self.base_entries()?;
            bounded_expanded_paths_under_with_selector(base_entries, path, max_paths, selector)?
        } else {
            BTreeSet::new()
        };
        let head_paths = if head_is_gitlink {
            let head_entries = self.head_entries()?;
            bounded_expanded_paths_under_with_selector(head_entries, path, max_paths, selector)?
        } else {
            BTreeSet::new()
        };
        let mut paths = base_paths.union(&head_paths).cloned().collect::<Vec<_>>();
        if include_base && !base_is_gitlink && selector.includes(path) {
            paths.push(path.to_owned());
        }
        if include_head && !head_is_gitlink && selector.includes(path) {
            paths.push(path.to_owned());
        }
        paths.sort();
        paths.dedup();

        Ok(Some(paths))
    }

    fn base_entries(&mut self) -> Result<&[GitTreeEntry], CodeIndexError> {
        if self.base_entries.is_none() {
            self.base_entries = Some(tracked_entries(self.root, &self.base_commit)?);
        }
        Ok(self.base_entries.as_deref().unwrap_or(&[]))
    }

    fn head_entries(&mut self) -> Result<&[GitTreeEntry], CodeIndexError> {
        if self.head_entries.is_none() {
            self.head_entries = Some(tracked_entries(self.root, &self.head_commit)?);
        }
        Ok(self.head_entries.as_deref().unwrap_or(&[]))
    }
}

pub(super) struct GitlinkPathExpansion {
    pub(super) base_is_gitlink: bool,
    pub(super) head_is_gitlink: bool,
    pub(super) base_paths: BTreeSet<String>,
    pub(super) head_paths: BTreeSet<String>,
}

struct SubmoduleChangedPathSets {
    base_paths: BTreeSet<String>,
    head_paths: BTreeSet<String>,
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
            root,
            path,
            base_gitlink,
            head_gitlink,
            max_paths,
            selector,
        )?
        else {
            return Ok(Some(GitlinkPathExpansion {
                base_is_gitlink: true,
                head_is_gitlink: true,
                base_paths: BTreeSet::new(),
                head_paths: BTreeSet::new(),
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
        Some(commit) => bounded_submodule_parent_paths(root, path, commit, max_paths, selector)?,
        None => BTreeSet::new(),
    };
    let head_paths = match &head_gitlink {
        Some(commit) => bounded_submodule_parent_paths(root, path, commit, max_paths, selector)?,
        None => BTreeSet::new(),
    };

    Ok(Some(GitlinkPathExpansion {
        base_is_gitlink: base_gitlink.is_some(),
        head_is_gitlink: head_gitlink.is_some(),
        base_paths,
        head_paths,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SubmodulePathEntry {
    pub(super) parent_path: String,
    pub(super) child_path: String,
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

pub(super) fn submodule_path_entries(
    root: &Path,
    path: &str,
    commit: &str,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    submodule_path_entries_with_scope(root, path, commit, &TrackedEntryScope::all())
}

pub(super) fn submodule_path_entries_with_child_filters(
    root: &Path,
    path: &str,
    commit: &str,
    child_filters: &[String],
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    submodule_path_entries_with_scope(
        root,
        path,
        commit,
        &TrackedEntryScope::from_entry_path_filters(child_filters.iter()),
    )
}

fn submodule_path_entries_with_scope(
    root: &Path,
    path: &str,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let entries = match submodule_worktree_root(root, path) {
        Ok(submodule_root) => tracked_entries_with_scope(&submodule_root, commit, scope)?,
        Err(_) => tracked_entries_from_git_dir_with_scope(
            &submodule_git_dir(root, path, None, Some(commit))?,
            commit,
            scope,
        )?,
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
    match submodule_worktree_root(root, path) {
        Ok(submodule_root) => git_blob_bytes_with_submodules(&submodule_root, commit, child_path),
        Err(_) => git_dir_blob_bytes_with_submodules(
            &submodule_git_dir(root, path, None, Some(commit))?,
            commit,
            child_path,
        ),
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
        root,
        path,
        &base_gitlink,
        &head_gitlink,
        max_paths,
        selector,
    )?
    else {
        return Ok(Some(BTreeSet::new()));
    };

    Ok(Some(
        changed_paths
            .base_paths
            .union(&changed_paths.head_paths)
            .cloned()
            .collect(),
    ))
}

fn changed_submodule_path_sets(
    root: &Path,
    path: &str,
    base_gitlink: &str,
    head_gitlink: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<SubmoduleChangedPathSets>, CodeIndexError> {
    changed_submodule_path_sets_inner(
        root,
        path,
        base_gitlink,
        head_gitlink,
        max_paths,
        0,
        selector,
    )
}

fn changed_submodule_path_sets_inner(
    root: &Path,
    path: &str,
    base_gitlink: &str,
    head_gitlink: &str,
    max_paths: usize,
    depth: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<Option<SubmoduleChangedPathSets>, CodeIndexError> {
    if base_gitlink == head_gitlink {
        return Ok(Some(SubmoduleChangedPathSets {
            base_paths: BTreeSet::new(),
            head_paths: BTreeSet::new(),
        }));
    }
    let changes = match diff_submodule_changes(root, path, base_gitlink, head_gitlink) {
        Ok(changes) => changes,
        Err(_) => return Ok(None),
    };
    let mut base_paths = BTreeSet::new();
    let mut head_paths = BTreeSet::new();
    let parent_path = path;
    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if !append_side_nested_gitlink_paths(
                    root,
                    parent_path,
                    base_gitlink,
                    &path,
                    &mut base_paths,
                    max_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut base_paths, parent_path, &path, selector);
                }
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                if !append_changed_nested_gitlink_paths(
                    NestedGitlinkChange {
                        root,
                        parent_path,
                        base_gitlink,
                        head_gitlink,
                        child_path: &path,
                        max_paths,
                        depth,
                    },
                    &mut base_paths,
                    &mut head_paths,
                    selector,
                )? {
                    let parent_child_path = parent_submodule_path(parent_path, &path);
                    if selector.includes(&parent_child_path) {
                        if submodule_blob_exists(root, parent_path, base_gitlink, &path)? {
                            base_paths.insert(parent_child_path.clone());
                        }
                        head_paths.insert(parent_child_path);
                    }
                }
            }
            GitChange::Renamed { old_path, new_path } => {
                if !append_side_nested_gitlink_paths(
                    root,
                    parent_path,
                    base_gitlink,
                    &old_path,
                    &mut base_paths,
                    max_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut base_paths, parent_path, &old_path, selector);
                }
                if !append_side_nested_gitlink_paths(
                    root,
                    parent_path,
                    head_gitlink,
                    &new_path,
                    &mut head_paths,
                    max_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut head_paths, parent_path, &new_path, selector);
                }
            }
            GitChange::Copied { new_path, .. } => {
                if !append_side_nested_gitlink_paths(
                    root,
                    parent_path,
                    head_gitlink,
                    &new_path,
                    &mut head_paths,
                    max_paths,
                    selector,
                )? {
                    insert_selected_parent_path(&mut head_paths, parent_path, &new_path, selector);
                }
            }
        }
        if base_paths.len().saturating_add(head_paths.len()) > max_paths {
            return Err(CodeIndexError::InvalidInput(format!(
                "gitlink path {path} expands to more than {max_paths} changed files; run a full code index so the work is checkpointed and batched"
            )));
        }
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
        change.base_gitlink,
        change.child_path,
    )?;
    let head_nested = submodule_gitlink_commit(
        change.root,
        change.parent_path,
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
                change.root,
                &nested_parent_path,
                base_commit,
                head_commit,
                change.max_paths,
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
            &commit,
            base_paths,
            change.max_paths,
            selector,
        )?,
        None if submodule_blob_exists(
            change.root,
            change.parent_path,
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

fn append_side_nested_gitlink_paths(
    root: &Path,
    parent_path: &str,
    parent_commit: &str,
    child_path: &str,
    paths: &mut BTreeSet<String>,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<bool, CodeIndexError> {
    let Some(nested_commit) =
        submodule_gitlink_commit(root, parent_path, parent_commit, child_path)?
    else {
        return Ok(false);
    };
    let nested_parent_path = parent_submodule_path(parent_path, child_path);
    if !selector.overlaps(&nested_parent_path) {
        return Ok(true);
    }
    append_bounded_submodule_entry_paths(
        root,
        &nested_parent_path,
        &nested_commit,
        paths,
        max_paths,
        selector,
    )?;

    Ok(true)
}

fn append_bounded_submodule_entry_paths(
    root: &Path,
    path: &str,
    commit: &str,
    paths: &mut BTreeSet<String>,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<(), CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(root, path, commit)?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<Vec<_>>();
    if selected.len() > max_paths {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            selected.len()
        )));
    }
    paths.extend(selected);

    Ok(())
}

fn submodule_gitlink_commit(
    root: &Path,
    path: &str,
    commit: &str,
    child_path: &str,
) -> Result<Option<String>, CodeIndexError> {
    match submodule_root(root, path) {
        Ok(submodule_root) => Ok(git_tree_entry(&submodule_root, commit, child_path)?
            .filter(|entry| entry.kind == "commit")
            .map(|entry| entry.object)),
        Err(_) => {
            let git_dir = submodule_git_dir(root, path, None, Some(commit))?;
            Ok(git_tree_entry_from_git_dir(&git_dir, commit, child_path)?
                .filter(|entry| entry.kind == "commit")
                .map(|entry| entry.object))
        }
    }
}

fn diff_submodule_changes(
    root: &Path,
    path: &str,
    base_gitlink: &str,
    head_gitlink: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    match submodule_root(root, path) {
        Ok(submodule_root) => diff_changes(&submodule_root, base_gitlink, head_gitlink),
        Err(_) => diff_changes_from_git_dir(
            &submodule_git_dir(root, path, None, Some(base_gitlink))?,
            base_gitlink,
            head_gitlink,
        ),
    }
}

fn submodule_blob_exists(
    root: &Path,
    path: &str,
    commit: &str,
    child_path: &str,
) -> Result<bool, CodeIndexError> {
    match submodule_root(root, path) {
        Ok(submodule_root) => Ok(git_bytes(
            &submodule_root,
            ["cat-file", "-e", &format!("{commit}:{child_path}")],
        )
        .is_ok()),
        Err(_) => Ok(git_dir_bytes(
            &submodule_git_dir(root, path, None, Some(commit))?,
            &["cat-file", "-e", &format!("{commit}:{child_path}")],
        )
        .is_ok()),
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
    commit: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let entries = submodule_path_entries_for_expansion(root, path, commit)?;
    let selected = entries
        .into_iter()
        .filter(|entry| selector.includes(&entry.parent_path))
        .map(|entry| entry.parent_path)
        .collect::<BTreeSet<_>>();
    if selected.len() > max_paths {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            selected.len()
        )));
    }

    Ok(selected)
}

fn submodule_path_entries_for_expansion(
    root: &Path,
    path: &str,
    commit: &str,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    match submodule_path_entries(root, path, commit) {
        Ok(entries) => Ok(entries),
        Err(error) if submodule_expansion_is_unavailable(&error) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn submodule_expansion_is_unavailable(error: &CodeIndexError) -> bool {
    matches!(
        error,
        CodeIndexError::InvalidInput(message)
            if message.contains("submodule git dir") && message.contains("unavailable")
    )
}

fn git_blob_bytes_with_submodules(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    match git_bytes(root, ["show", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(bytes),
        Err(error) => submodule_bytes(root, commit, path).map_err(|_| error),
    }
}

fn git_dir_blob_bytes_with_submodules(
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
    if !safe_relative_path(path) {
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
            Ok(submodule_root) => GitlinkTargetLocation::Worktree(submodule_root),
            Err(_) => GitlinkTargetLocation::GitDir(submodule_git_dir(
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
    if !safe_relative_path(path) {
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

#[derive(Debug)]
struct GitTreeLookup {
    object: String,
    kind: String,
}

fn git_tree_entry(
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

fn git_tree_entry_from_git_dir(
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
