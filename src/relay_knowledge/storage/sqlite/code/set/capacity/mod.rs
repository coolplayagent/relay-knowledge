//! Fixed resource ceilings for repository-set membership and overlay refresh.

use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(in crate::storage::sqlite::code) const MAX_REPOSITORY_SET_MEMBERS: usize = 64;
pub(super) const MAX_REPOSITORY_SET_OVERLAY_IMPORTS: usize = 8_192;
pub(super) const MAX_REPOSITORY_SET_OVERLAY_EXPORTS: usize = 131_072;
pub(in crate::storage::sqlite::code) const MAX_REPOSITORY_SET_OVERLAY_EDGES: usize = 8_192;
pub(super) const MAX_REPOSITORY_SET_MANIFEST_CHUNKS: usize = 4_096;
pub(super) const MAX_REPOSITORY_SET_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_REPOSITORY_SET_MANIFEST_ITEMS: usize = 32_768;
pub(super) const MAX_MATCHING_EXPORTS_PER_IMPORT: usize = 11;
pub(super) const MAX_OVERLAY_EDGE_SELECTOR_KEYS: usize = 512;

pub(in crate::storage::sqlite::code) fn ensure_overlay_delete_is_bounded(
    connection: &Connection,
    set_id: &str,
) -> Result<(), StorageError> {
    let observed = connection.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM code_repository_cross_edges
             WHERE set_id = ?1
             LIMIT ?2
         )",
        params![set_id, MAX_REPOSITORY_SET_OVERLAY_EDGES + 1],
        |row| row.get::<_, usize>(0),
    )?;
    if observed > MAX_REPOSITORY_SET_OVERLAY_EDGES {
        return Err(StorageError::CapacityExceeded(format!(
            "repository-set overlay '{set_id}' exceeds the bounded delete capacity of \
             {MAX_REPOSITORY_SET_OVERLAY_EDGES} edges; this legacy state requires an upgrade or \
             bounded repair tool before refresh or membership changes can continue"
        )));
    }
    Ok(())
}

pub(super) fn capacity_error(kind: &str, capacity: usize) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "repository-set overlay exceeds the {kind} capacity of {capacity}; narrow the set or \
         indexed scopes before retrying"
    ))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
