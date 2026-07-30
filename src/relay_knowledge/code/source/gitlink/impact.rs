use std::{collections::BTreeSet, path::Path};

use super::{
    diff::{SubmoduleDiffRequest, bounded_submodule_parent_paths, changed_submodule_path_sets},
    entries::gitlink_commit_at_tree,
    paths::{GitlinkPathExpansion, ensure_gitlink_expansion_budget},
    selector::GitlinkPathSelector,
};
use crate::code::CodeIndexError;

#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;

pub(in crate::code) struct GitlinkImpactExpander<'a> {
    root: &'a Path,
    base_commit: String,
    head_commit: String,
    max_paths: usize,
}

impl<'a> GitlinkImpactExpander<'a> {
    pub(in crate::code) fn new(
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

    pub(in crate::code) fn expanded_paths(
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
                None,
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
                None,
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

pub(in crate::code) fn changed_gitlink_path_expansion(
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
                git_dir: None,
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
                None,
                base_parent_commit,
                base_gitlink,
                max_paths,
                selector,
            )?;
            let head_paths = bounded_submodule_parent_paths(
                root,
                path,
                None,
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
            None,
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
            None,
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
            git_dir: None,
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
            None,
            base_parent_commit,
            &base_gitlink,
            max_paths,
            selector,
        )?;
        paths.extend(bounded_submodule_parent_paths(
            root,
            path,
            None,
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
