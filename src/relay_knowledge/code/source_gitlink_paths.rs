use std::collections::BTreeSet;

use super::{CodeIndexError, changes::GitTreeEntry, source_gitlink_selector::GitlinkPathSelector};

pub(super) fn expanded_paths_under(entries: &[GitTreeEntry], path: &str) -> BTreeSet<String> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    entries
        .iter()
        .filter(|entry| entry.path.starts_with(&prefix))
        .map(|entry| entry.path.clone())
        .collect()
}

pub(super) fn bounded_expanded_paths_under(
    entries: &[GitTreeEntry],
    path: &str,
    max_paths: usize,
) -> Result<BTreeSet<String>, CodeIndexError> {
    bounded_expanded_paths_under_with_selector(
        entries,
        path,
        max_paths,
        &GitlinkPathSelector::all(),
    )
}

pub(super) fn bounded_expanded_paths_under_with_selector(
    entries: &[GitTreeEntry],
    path: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let paths = expanded_paths_under(entries, path)
        .into_iter()
        .filter(|path| selector.includes(path))
        .collect::<BTreeSet<_>>();
    if paths.len() > max_paths {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            paths.len()
        )));
    }

    Ok(paths)
}
