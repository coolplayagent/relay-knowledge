use rusqlite::Transaction;

use crate::{domain::CodeMonorepoWorkspace, storage::StorageError};

mod cross_edge;
mod ecosystem;
mod mapping;
mod member_target;
mod set_state;

pub(super) use set_state::{
    clear_auto_workspace_state, clear_auto_workspace_state_with_fence, clear_workspace_state,
    has_auto_workspace_state, workspace_set_id,
};

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

/// Resolves unresolved imports against workspace package mappings and
/// creates cross-repository edges in `code_repository_cross_edges`.
///
/// Empty `workspaces` clears any previous auto-detected workspace state so
/// a later index cannot keep stale package mappings or generated edges.
pub(crate) fn resolve_workspace_imports(
    transaction: &Transaction<'_>,
    workspaces: &[CodeMonorepoWorkspace],
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if workspaces.is_empty() {
        clear_workspace_state(transaction, repository_id, source_scope)?;
        return Ok(());
    }

    let now = crate::clock::system_now_millis_or_zero();
    let set = set_state::ensure_workspace_set(transaction, repository_id, source_scope, now)?;
    mapping::replace_workspace_package_mappings(
        transaction,
        workspaces,
        &set,
        repository_id,
        source_scope,
        now,
    )?;
    cross_edge::replace_workspace_cross_edges(
        transaction,
        workspaces,
        &set,
        repository_id,
        source_scope,
        now,
    )?;
    set_state::refresh_workspace_overlay_status(transaction, &set.set_id, now)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
