pub(in crate::code) mod commands;
mod diff;
mod entries;
mod impact;
pub(in crate::code) mod paths;
pub(in crate::code) mod selector;
pub(in crate::code) mod target;

pub(in crate::code) use entries::{
    gitlink_commit_at_tree, submodule_entry_bytes, submodule_path_entries_with_child_filters,
    submodule_root,
};
pub(in crate::code) use impact::{GitlinkImpactExpander, changed_gitlink_path_expansion};
pub(in crate::code) use paths::{
    SubmodulePathEntry, ensure_gitlink_expansion_budget, submodule_expansion_is_unavailable,
};
pub(in crate::code) use selector::GitlinkPathSelector;
pub(in crate::code) use target::{submodule_blob_size, submodule_bytes};
