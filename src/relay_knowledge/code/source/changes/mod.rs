pub(in crate::code) use super::change_status::{
    GitChange, WorktreePathChange, parse_name_status_z, worktree_changed_paths,
};

mod diff;
mod scope;
mod submodule_repository;
mod tracked_entries;

pub(in crate::code) use self::diff::{MAX_GIT_DIFF_CHANGED_PATHS, diff_changes};
pub(in crate::code) use self::scope::TrackedEntryScope;
pub(in crate::code) use self::submodule_repository::{
    submodule_git_dir, submodule_git_dir_from_git_dir, submodule_worktree_root,
};
#[cfg(test)]
pub(in crate::code) use self::tracked_entries::tracked_entries;
pub(in crate::code) use self::tracked_entries::{
    GitTreeEntry, tracked_entries_from_git_dir_with_scope, tracked_entries_state_with_scope,
    tracked_entries_with_scope,
};
#[cfg(test)]
pub(crate) use self::tracked_entries::{
    reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root,
};
