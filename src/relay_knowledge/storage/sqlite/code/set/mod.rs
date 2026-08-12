mod capacity;
mod manifest;
mod membership;
mod overlay;
pub(super) mod refresh_tasks;

#[cfg(test)]
pub(in crate::storage::sqlite::code) use capacity::MAX_REPOSITORY_SET_OVERLAY_EDGES;
pub(in crate::storage::sqlite::code) use capacity::{
    MAX_REPOSITORY_SET_MEMBERS, ensure_overlay_delete_is_bounded,
};
pub(super) use membership::{add_member, create_set, remove_member, set_by_alias};
pub(super) use overlay::{
    cross_edges_for_selector, cross_edges_for_set, refresh_overlay_for_task, set_status,
};

#[cfg(test)]
pub(super) use overlay::refresh_overlay;

#[cfg(test)]
mod tests;
