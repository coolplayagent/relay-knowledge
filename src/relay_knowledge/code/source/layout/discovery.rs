use std::collections::BTreeSet;

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::path_scope::{
    intersect_path_filters, merged_path_filters, normalize_path_filter, normalized_path_filters,
    path_matches_filter, path_overlaps_filter, push_filter_if_uncovered,
};
use crate::code::source::{
    changes::GitTreeEntry,
    filesystem::{source_default_file_preset_excludes, source_path_has_indexable_content},
    roots::{NESTED_SOURCE_MARKERS, STRIPPABLE_SOURCE_ROOTS},
};

const SOURCE_LAYOUT_DISCOVERY_MAX_PATHS: usize = 200_000;
const SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS: usize = 512;
const AUTO_SOURCE_SCOPE_FILTERS: &[&str] = &[".", "src", "include", "lib", "Sources"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::code) struct SourceLayoutDiscovery {
    source_roots: BTreeSet<String>,
}

impl SourceLayoutDiscovery {
    pub(super) fn keeps_default_excluded_source(&self, path: &str) -> bool {
        source_path_has_indexable_content(path)
            && !path_contains_broad_dependency_segment(path)
            && self
                .source_roots
                .iter()
                .any(|root| path_matches_filter(path, root))
    }

    pub(super) fn extends_path_scope(
        &self,
        path: &str,
        registration: &CodeRepositoryRegistration,
        selector: &CodeRepositorySelector,
    ) -> bool {
        registration_scope_can_discover_source_roots(&registration.path_filters)
            && selector_path_scope_allows_discovered_root(path, &selector.path_filters)
            && self.keeps_default_excluded_source(path)
    }
}

pub(in crate::code) fn discover_source_layout(entries: &[GitTreeEntry]) -> SourceLayoutDiscovery {
    let mut source_roots = BTreeSet::new();
    for entry in entries.iter().take(SOURCE_LAYOUT_DISCOVERY_MAX_PATHS) {
        if !source_path_has_indexable_content(&entry.path)
            || path_contains_broad_dependency_segment(&entry.path)
            || source_default_file_preset_excludes(&entry.path)
        {
            continue;
        }
        for root in source_layout_roots_for_path(&entry.path) {
            source_roots.insert(root);
            if source_roots.len() >= SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS {
                return SourceLayoutDiscovery { source_roots };
            }
        }
    }

    SourceLayoutDiscovery { source_roots }
}

pub(in crate::code) fn effective_index_path_filters(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> Vec<String> {
    effective_index_path_filters_for_layouts(registration, selector, &[source_layout])
}

pub(in crate::code) fn effective_index_path_filters_for_layouts(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layouts: &[&SourceLayoutDiscovery],
) -> Vec<String> {
    let mut filters = merged_path_filters(&registration.path_filters, &selector.path_filters);
    if !registration_scope_can_discover_source_roots(&registration.path_filters) {
        return filters;
    }
    for source_layout in source_layouts {
        for root in &source_layout.source_roots {
            if !selector_filter_allows_root(root, &selector.path_filters) {
                continue;
            }
            push_filter_if_uncovered(&mut filters, root);
        }
    }

    filters
}

pub(in crate::code) fn effective_path_filter_intersections_for_layouts(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layouts: &[&SourceLayoutDiscovery],
) -> Option<Vec<String>> {
    let mut filters = normalized_path_filters(&registration.path_filters);
    if registration_scope_can_discover_source_roots(&registration.path_filters) {
        for source_layout in source_layouts {
            for root in &source_layout.source_roots {
                if selector_filter_allows_root(root, &selector.path_filters) {
                    push_filter_if_uncovered(&mut filters, root);
                }
            }
        }
    }

    intersect_path_filters(&filters, &selector.path_filters)
}

fn path_contains_broad_dependency_segment(path: &str) -> bool {
    normalize_path_filter(path)
        .split('/')
        .any(|segment| matches!(segment, "vendor" | "third_party" | "node_modules"))
}

fn registration_scope_can_discover_source_roots(filters: &[String]) -> bool {
    !filters.is_empty()
        && filters.iter().all(|filter| {
            let filter = normalize_path_filter(filter);
            AUTO_SOURCE_SCOPE_FILTERS.contains(&filter)
        })
}

fn selector_path_scope_allows_discovered_root(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

fn selector_filter_allows_root(root: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(root, filter) || path_overlaps_filter(root, filter))
}

fn source_layout_roots_for_path(path: &str) -> Vec<String> {
    let path = normalize_path_filter(path);
    let mut roots = Vec::new();
    if path_matches_filter(path, "include") {
        push_source_root(&mut roots, "include".to_owned());
    }
    for marker in NESTED_SOURCE_MARKERS {
        if let Some((prefix, _)) = path.split_once(marker) {
            push_source_root(&mut roots, format!("{prefix}{marker}"));
        }
    }
    for root in STRIPPABLE_SOURCE_ROOTS {
        if let Some(suffix) = path.strip_prefix(root) {
            let mut segments = suffix.split('/').filter(|segment| !segment.is_empty());
            if let Some(first) = segments.next() {
                push_source_root(&mut roots, format!("{root}{first}"));
            } else {
                push_source_root(&mut roots, root.trim_end_matches('/').to_owned());
            }
        }
    }
    roots
}

fn push_source_root(roots: &mut Vec<String>, root: String) {
    let root = root.trim_end_matches('/').to_owned();
    if !root.is_empty() && !roots.contains(&root) {
        roots.push(root);
    }
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
