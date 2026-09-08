//! Additive code-schema capability checks used by warm-open admission.
use super::introspection::{index_has_columns, table_exists, table_has_columns};
use crate::storage::StorageError;
use rusqlite::{Connection, params};

pub(in crate::storage::sqlite) const SEARCH_OWNER_V2_MIGRATION: &str =
    "search-owner-v2-writer-and-serving-gate";
pub(in crate::storage::sqlite) const REFERENCE_SEARCH_GROUP_V2_MIGRATION: &str =
    "reference-search-group-owner-v2";
pub(in crate::storage::sqlite) const SEARCH_ORPHAN_GC_PHASE_MIGRATION: &str =
    "scope-gc-search-orphans-phase-v1";
pub(in crate::storage::sqlite) const REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION: &str =
    "scope-gc-reference-search-groups-phase-v1";
pub(super) fn code_schema_capability_markers_are_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !table_exists(connection, "code_repository_schema_migrations")?
        || !table_has_columns(
            connection,
            "code_repository_feature_flags",
            &["metadata_json"],
        )?
        || !index_has_columns(
            connection,
            "code_repository_feature_flags_source_key",
            &["source_scope", "source_kind", "source_key"],
        )?
    {
        return Ok(false);
    }
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?1
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?2
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?3
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?4
             )",
            params![
                SEARCH_OWNER_V2_MIGRATION,
                SEARCH_ORPHAN_GC_PHASE_MIGRATION,
                REFERENCE_SEARCH_GROUP_V2_MIGRATION,
                REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "code_capabilities_tests.rs"]
mod tests;
