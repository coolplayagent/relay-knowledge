use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::{discovery::SourceLayoutDiscovery, path_scope::path_scope_allows};
use crate::code::{
    parser::dependency_manifest_overrides_default_exclusion,
    source::{
        filesystem::{
            explicit_path_filter_opts_into_default_file_exclusion,
            filesystem_default_source_allows, source_default_file_preset_excludes,
        },
        repository::{RepositorySourceKind, source_language_filter_allows},
    },
};

#[cfg(test)]
pub(in crate::code) fn path_is_selected(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    selection_exclusion_reason(path, registration, selector).is_none()
}

pub(in crate::code) fn path_is_selected_with_layout(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> bool {
    selection_exclusion_reason_with_layout(path, registration, selector, source_layout).is_none()
}

#[cfg(test)]
pub(in crate::code) fn selection_exclusion_reason(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Option<String> {
    selection_exclusion_reason_with_layout(
        path,
        registration,
        selector,
        &SourceLayoutDiscovery::default(),
    )
}

pub(in crate::code) fn selection_exclusion_reason_with_layout(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> Option<String> {
    selection_exclusion_reason_for_source(
        path,
        registration,
        selector,
        source_layout,
        RepositorySourceKind::Git,
    )
}

pub(in crate::code) fn selection_exclusion_reason_for_source(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
    source_kind: RepositorySourceKind,
) -> Option<String> {
    if !path_scope_allows(path, registration, selector)
        && !source_layout.extends_path_scope(path, registration, selector)
    {
        return Some("outside registered/requested path scope".to_owned());
    }
    if source_kind.is_filesystem()
        && filesystem_default_scope_excludes(path, registration, selector)
    {
        return Some("outside non-git default source whitelist".to_owned());
    }
    if !source_language_filter_allows(path, &registration.language_filters)
        || !source_language_filter_allows(path, &selector.language_filters)
    {
        return Some("outside registered/requested language scope".to_owned());
    }
    if source_default_file_preset_excludes(path)
        && !dependency_manifest_overrides_default_exclusion(path)
        && !source_layout.keeps_default_excluded_source(path)
        && !explicit_path_filter_opts_into_default_file_exclusion(
            path,
            registration
                .path_filters
                .iter()
                .chain(selector.path_filters.iter()),
        )
    {
        return Some("excluded by file preset".to_owned());
    }

    None
}

fn filesystem_default_scope_excludes(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    if !registration.path_filters.is_empty() || !selector.path_filters.is_empty() {
        return false;
    }

    !filesystem_default_source_allows(path)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
