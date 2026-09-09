//! Bounded incremental blob prefetch and aggregate origin plan admission.
use super::*;
pub(super) struct ChangedPathPrefetchRequest<'a> {
    pub(super) reparse_python: bool,
    pub(super) registration: &'a CodeRepositoryRegistration,
    pub(super) selector: &'a CodeRepositorySelector,
    pub(super) root: &'a Path,
    pub(super) commit: &'a str,
    pub(super) changes: &'a [GitChange],
    pub(super) head_entries: &'a [changes::GitTreeEntry],
    pub(super) source_layout: &'a scope::SourceLayoutDiscovery,
    pub(super) previous_source_layout: &'a scope::SourceLayoutDiscovery,
}

pub(super) fn prefetch_changed_path_bytes(
    request: ChangedPathPrefetchRequest<'_>,
) -> Result<BTreeMap<String, Vec<u8>>, CodeIndexError> {
    if request.reparse_python {
        validate_origin_plan(&request)?;
    }
    let entries = request
        .head_entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.byte_count))
        .collect::<BTreeMap<_, _>>();
    let budget = CodeIndexResourceBudget::default();
    let mut paths = Vec::new();
    let mut total_bytes = 0usize;
    for path in request.changes.iter().filter_map(changed_head_path) {
        let Some(byte_count) = entries.get(path).copied() else {
            continue;
        };
        if !path_is_selected_with_layout(
            path,
            request.registration,
            request.selector,
            request.source_layout,
        ) && !path_is_selected_with_layout(
            path,
            request.registration,
            request.selector,
            request.previous_source_layout,
        ) {
            continue;
        }
        if paths.iter().any(|selected| selected == path) {
            continue;
        }
        if !paths.is_empty()
            && (paths.len() >= budget.max_files_per_batch
                || total_bytes.saturating_add(byte_count) > budget.max_bytes_per_batch)
        {
            break;
        }
        total_bytes = total_bytes.saturating_add(byte_count);
        paths.push(path.to_owned());
    }
    let blobs =
        source_batch_bytes_after_content_verification(request.root, request.commit, &paths, None)?;

    Ok(paths.into_iter().zip(blobs).collect())
}

fn changed_head_path(change: &GitChange) -> Option<&str> {
    match change {
        GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => Some(path),
        GitChange::Renamed { new_path, .. } | GitChange::Copied { new_path, .. } => Some(new_path),
        GitChange::Deleted { .. } => None,
    }
}

fn validate_origin_plan(request: &ChangedPathPrefetchRequest<'_>) -> Result<(), CodeIndexError> {
    let changed = request
        .changes
        .iter()
        .filter_map(changed_head_path)
        .collect::<BTreeSet<_>>();
    let mut budget = super::super::origin_reparse_budget::OriginReparseBudget::default();
    for entry in request.head_entries {
        if changed.contains(entry.path.as_str())
            && (path_is_selected_with_layout(
                &entry.path,
                request.registration,
                request.selector,
                request.source_layout,
            ) || path_is_selected_with_layout(
                &entry.path,
                request.registration,
                request.selector,
                request.previous_source_layout,
            ))
        {
            budget.charge(entry.byte_count)?;
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "prefetch_tests.rs"]
mod tests;
