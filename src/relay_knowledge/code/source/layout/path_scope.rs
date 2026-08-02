use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

pub(super) fn merged_path_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for filter in left.iter().chain(right.iter()) {
        let normalized = normalize_path_filter(filter);
        if !normalized.is_empty() && !merged.iter().any(|existing| existing == normalized) {
            merged.push(normalized.to_owned());
        }
    }

    merged
}

pub(in crate::code) fn intersect_path_filters(
    left: &[String],
    right: &[String],
) -> Option<Vec<String>> {
    let left = normalized_path_filters(left);
    let right = normalized_path_filters(right);
    if left.is_empty() {
        return Some(right);
    }
    if right.is_empty() {
        return Some(left);
    }

    let mut intersections = Vec::new();
    for left_filter in &left {
        for right_filter in &right {
            if path_filter_covers(left_filter, right_filter) {
                push_filter_if_missing(&mut intersections, right_filter);
            } else if path_filter_covers(right_filter, left_filter) {
                push_filter_if_missing(&mut intersections, left_filter);
            }
        }
    }

    (!intersections.is_empty()).then_some(intersections)
}

pub(in crate::code) fn submodule_child_scope_filters(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Option<Vec<String>> {
    let filters = intersect_path_filters(&registration.path_filters, &selector.path_filters)?;
    submodule_child_scope_filters_from_filters(path, &filters)
}

pub(in crate::code) fn submodule_child_scope_filters_from_filters(
    path: &str,
    filters: &[String],
) -> Option<Vec<String>> {
    if filters.is_empty() {
        return Some(Vec::new());
    }
    let path = normalize_scope_path(path);
    if path.is_empty() {
        return None;
    }
    let child_prefix = format!("{path}/");
    let mut child_filters = Vec::new();
    let mut parent_scope_covers_submodule = false;
    for filter in filters {
        let filter = normalize_scope_path(filter);
        if filter.is_empty()
            || filter == "."
            || filter == path
            || path.starts_with(&format!("{filter}/"))
        {
            parent_scope_covers_submodule = true;
            continue;
        }
        if let Some(child_filter) = filter.strip_prefix(&child_prefix)
            && !child_filter.is_empty()
        {
            child_filters.push(child_filter.to_owned());
        }
    }
    if parent_scope_covers_submodule {
        return Some(Vec::new());
    }
    if child_filters.is_empty() {
        return None;
    }
    child_filters.sort();
    child_filters.dedup();

    Some(child_filters)
}

pub(super) fn push_filter_if_uncovered(filters: &mut Vec<String>, root: &str) {
    if filters
        .iter()
        .any(|filter| path_filter_covers(filter, root))
    {
        return;
    }
    filters.retain(|filter| !path_filter_covers(root, filter));
    filters.push(root.to_owned());
}

pub(in crate::code) fn path_scope_allows(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    path_filter_allows(path, &registration.path_filters)
        && path_filter_allows(path, &selector.path_filters)
}

pub(in crate::code) fn path_scope_overlaps(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    path_filter_overlaps(path, &registration.path_filters)
        && path_filter_overlaps(path, &selector.path_filters)
}

pub(in crate::code) fn path_overlaps_any_filter(path: &str, filters: &[String]) -> bool {
    path_filter_overlaps(path, filters)
}

fn normalize_scope_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_owned()
}

pub(super) fn normalized_path_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|filter| normalize_path_filter(filter).to_owned())
        .filter(|filter| !filter.is_empty())
        .collect()
}

fn push_filter_if_missing(filters: &mut Vec<String>, filter: &str) {
    if !filters.iter().any(|existing| existing == filter) {
        filters.push(filter.to_owned());
    }
}

fn path_filter_covers(filter: &str, path: &str) -> bool {
    let filter = normalize_path_filter(filter);
    filter == "." || path_matches_filter(path, filter)
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

fn path_filter_overlaps(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_overlaps_filter(path, filter))
}

pub(super) fn path_matches_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

pub(super) fn path_overlaps_filter(path: &str, filter: &str) -> bool {
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

pub(super) fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

#[cfg(test)]
#[path = "path_scope_tests.rs"]
mod tests;
