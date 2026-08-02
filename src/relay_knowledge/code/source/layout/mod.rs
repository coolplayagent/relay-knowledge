mod discovery;
mod impact_partition;
mod path_scope;
mod preview;
mod scoped_snapshot;
mod selection;

pub(in crate::code) use self::discovery::{
    SourceLayoutDiscovery, discover_source_layout, effective_index_path_filters,
    effective_index_path_filters_for_layouts, effective_path_filter_intersections_for_layouts,
};
pub use self::impact_partition::partition_changed_paths_for_selector;
#[cfg(test)]
pub(in crate::code) use self::path_scope::path_scope_allows;
pub(in crate::code) use self::path_scope::{
    intersect_path_filters, path_overlaps_any_filter, path_scope_overlaps,
    submodule_child_scope_filters, submodule_child_scope_filters_from_filters,
};
pub use self::preview::preview_repository_scope;
pub(in crate::code) use self::scoped_snapshot::{
    ScopedSourceSnapshot, filesystem_policy_for_selector, scoped_source_snapshot,
    scoped_source_snapshot_for_filters, scoped_source_snapshot_for_registration,
    scoped_source_snapshot_for_registration_filters,
};
#[cfg(test)]
pub(in crate::code) use self::selection::path_is_selected;
pub(in crate::code) use self::selection::{
    path_is_selected_with_layout, selection_exclusion_reason_for_source,
};
