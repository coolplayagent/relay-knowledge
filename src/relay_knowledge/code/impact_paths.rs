use std::path::Path;

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    CodeIndexError, MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, changes::GitChange, git::resolve_ref,
    scope, scope::path_is_selected_with_layout, source_gitlink,
};

pub(super) fn paths_from_changes_with_gitlinks(
    root: &Path,
    base_ref: &str,
    head_ref: &str,
    changes: Vec<GitChange>,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    let base_commit = resolve_ref(root, base_ref)?;
    let head_commit = resolve_ref(root, head_ref)?;
    let impact_registration = CodeRepositoryRegistration {
        repository_id: "impact".to_owned(),
        alias: "impact".to_owned(),
        root_path: root.display().to_string(),
        path_filters: path_filters.to_vec(),
        language_filters: Vec::new(),
    };
    let impact_selector = CodeRepositorySelector {
        repository: "impact".to_owned(),
        ref_selector: head_ref.to_owned(),
        path_filters: Vec::new(),
        language_filters: language_filters.to_vec(),
    };
    let source_layout = scope::SourceLayoutDiscovery::default();
    let mut expander = source_gitlink::GitlinkImpactExpander::new(
        root,
        base_commit,
        head_commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    );
    let mut paths = Vec::new();
    for change in changes {
        match change {
            GitChange::Deleted { path } => push_impact_paths(ImpactPathPush {
                paths: &mut paths,
                expander: &mut expander,
                path_filters,
                registration: &impact_registration,
                selector: &impact_selector,
                source_layout: &source_layout,
                include_base: true,
                include_head: false,
                path: &path,
            })?,
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                push_impact_paths(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    registration: &impact_registration,
                    selector: &impact_selector,
                    source_layout: &source_layout,
                    include_base: true,
                    include_head: true,
                    path: &path,
                })?
            }
            GitChange::Renamed { old_path, new_path } => {
                push_impact_paths(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    registration: &impact_registration,
                    selector: &impact_selector,
                    source_layout: &source_layout,
                    include_base: true,
                    include_head: false,
                    path: &old_path,
                })?;
                push_impact_paths(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    registration: &impact_registration,
                    selector: &impact_selector,
                    source_layout: &source_layout,
                    include_base: false,
                    include_head: true,
                    path: &new_path,
                })?;
            }
            GitChange::Copied { new_path, .. } => push_impact_paths(ImpactPathPush {
                paths: &mut paths,
                expander: &mut expander,
                path_filters,
                registration: &impact_registration,
                selector: &impact_selector,
                source_layout: &source_layout,
                include_base: false,
                include_head: true,
                path: &new_path,
            })?,
        }
    }
    paths.sort();
    paths.dedup();

    Ok(paths)
}

struct ImpactPathPush<'a, 'b> {
    paths: &'a mut Vec<String>,
    expander: &'a mut source_gitlink::GitlinkImpactExpander<'b>,
    path_filters: &'a [String],
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    source_layout: &'a scope::SourceLayoutDiscovery,
    include_base: bool,
    include_head: bool,
    path: &'a str,
}

fn push_impact_paths(request: ImpactPathPush<'_, '_>) -> Result<(), CodeIndexError> {
    if !path_scope_overlaps(request.path, request.path_filters) {
        request.paths.push(request.path.to_owned());
        return Ok(());
    }
    let include_path = |path: &str| {
        path_is_selected_with_layout(
            path,
            request.registration,
            request.selector,
            request.source_layout,
        )
    };
    let scope_overlaps = |path: &str| path_scope_overlaps(path, request.path_filters);
    match request.expander.expanded_paths(
        request.path,
        request.include_base,
        request.include_head,
        &source_gitlink::GitlinkPathSelector::new(&include_path, &scope_overlaps),
    )? {
        Some(expanded) if !expanded.is_empty() => request.paths.extend(expanded),
        _ => request.paths.push(request.path.to_owned()),
    }

    Ok(())
}

fn path_scope_overlaps(path: &str, path_filters: &[String]) -> bool {
    path_filters.is_empty()
        || path_filters
            .iter()
            .any(|filter| path_overlaps_filter(path, filter))
}

fn path_overlaps_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !path.is_empty()
        && !filter.is_empty()
        && (path == filter
            || path.starts_with(&format!("{filter}/"))
            || filter.starts_with(&format!("{path}/")))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
