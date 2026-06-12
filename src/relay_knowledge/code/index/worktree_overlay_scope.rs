use std::collections::BTreeMap;

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    CodeIndexError, MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, changes, scope,
    worktree_overlay_untracked as untracked,
};

pub(super) struct WorktreeOverlayScope<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    source_layout: scope::SourceLayoutDiscovery,
    pub(super) path_filters: Vec<String>,
    pub(super) selection_path_filters: Option<Vec<String>>,
}

impl<'a> WorktreeOverlayScope<'a> {
    pub(super) fn new(
        registration: &'a CodeRepositoryRegistration,
        selector: &'a CodeRepositorySelector,
        previous_hashes: &BTreeMap<String, String>,
    ) -> Self {
        let previous_entries = previous_hashes
            .keys()
            .map(|path| changes::GitTreeEntry {
                path: path.clone(),
                byte_count: 0,
            })
            .collect::<Vec<_>>();
        let source_layout = scope::discover_source_layout(&previous_entries);
        let path_filters = scope::effective_index_path_filters_for_layouts(
            registration,
            selector,
            &[&source_layout],
        );
        let selection_path_filters = scope::effective_path_filter_intersections_for_layouts(
            registration,
            selector,
            &[&source_layout],
        );
        Self {
            registration,
            selector,
            source_layout,
            path_filters,
            selection_path_filters,
        }
    }

    pub(super) fn selected(&self, path: &str) -> bool {
        scope::path_is_selected_with_layout(
            path,
            self.registration,
            self.selector,
            &self.source_layout,
        )
    }

    pub(super) fn overlaps(&self, path: &str) -> bool {
        self.selection_path_filters
            .as_ref()
            .is_some_and(|filters| scope::path_overlaps_any_filter(path, filters))
    }

    pub(super) fn untracked_selected(&self, path: &str) -> bool {
        self.selected(path) && untracked::allowed(path, self.registration, self.selector)
    }
}

pub(super) fn bounded_worktree_changes(
    changes: Vec<changes::WorktreePathChange>,
    scope: &WorktreeOverlayScope<'_>,
) -> Result<Vec<changes::WorktreePathChange>, CodeIndexError> {
    let scoped_count = changes
        .iter()
        .filter(|change| worktree_change_touches_recordable_scope(change, scope))
        .count();
    if scoped_count > MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "worktree overlay changes exceed {MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS} paths; commit changes, run a full code index, or narrow --path before indexing --ref worktree"
        )));
    }

    Ok(changes)
}

fn worktree_change_touches_recordable_scope(
    change: &changes::WorktreePathChange,
    scope: &WorktreeOverlayScope<'_>,
) -> bool {
    change
        .deleted_source
        .as_deref()
        .is_some_and(|path| scope.overlaps(path) || scope.selected(path))
        || if change.is_untracked() {
            scope.untracked_selected(&change.path)
        } else {
            scope.overlaps(&change.path) || scope.selected(&change.path)
        }
}
