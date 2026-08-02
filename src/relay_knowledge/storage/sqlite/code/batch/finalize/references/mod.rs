//! Normalizes and resolves ordinary code references during finalization.

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn resolve(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        WITH reference_names AS (
            SELECT DISTINCT name
            FROM code_repository_references
            WHERE source_scope = ?1 AND kind != 'call'
        ),
        unique_symbol AS (
            SELECT name, MIN(symbol_snapshot_id) AS symbol_snapshot_id
            FROM code_repository_symbols
            WHERE source_scope = ?1
              AND name IN (SELECT name FROM reference_names)
            GROUP BY name
            HAVING COUNT(*) = 1
        )
        UPDATE code_repository_references AS reference
        SET target_symbol_snapshot_id = unique_symbol.symbol_snapshot_id,
            resolution_state = 'resolved',
            confidence_basis_points = 8000,
            confidence_tier = 'inferred'
        FROM unique_symbol
        WHERE reference.source_scope = ?1
          AND reference.kind != 'call'
          AND reference.name = unique_symbol.name
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        WITH reference_pairs AS (
            SELECT DISTINCT name, path
            FROM code_repository_references
            WHERE source_scope = ?1
              AND kind != 'call'
              AND resolution_state != 'resolved'
        ),
        unique_path_symbol AS (
            SELECT name, path, MIN(symbol_snapshot_id) AS symbol_snapshot_id
            FROM code_repository_symbols
            WHERE source_scope = ?1
              AND (name, path) IN (SELECT name, path FROM reference_pairs)
            GROUP BY name, path
            HAVING COUNT(*) = 1
        )
        UPDATE code_repository_references AS reference
        SET target_symbol_snapshot_id = unique_path_symbol.symbol_snapshot_id,
            resolution_state = 'resolved',
            confidence_basis_points = 8000,
            confidence_tier = 'inferred'
        FROM unique_path_symbol
        WHERE reference.source_scope = ?1
          AND reference.kind != 'call'
          AND reference.resolution_state != 'resolved'
          AND reference.name = unique_path_symbol.name
          AND reference.path = unique_path_symbol.path
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        WITH reference_names AS (
            SELECT DISTINCT name
            FROM code_repository_references
            WHERE source_scope = ?1
              AND kind != 'call'
              AND resolution_state = 'unresolved'
        ),
        symbol_names AS (
            SELECT DISTINCT name
            FROM code_repository_symbols
            WHERE source_scope = ?1
              AND name IN (SELECT name FROM reference_names)
        )
        UPDATE code_repository_references AS reference
        SET resolution_state = 'ambiguous',
            confidence_basis_points = 5000,
            confidence_tier = 'ambiguous'
        FROM symbol_names
        WHERE reference.source_scope = ?1
          AND reference.kind != 'call'
          AND reference.resolution_state = 'unresolved'
          AND reference.name = symbol_names.name
        ",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn normalize_unresolved(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_references
        SET target_symbol_snapshot_id = NULL,
            target_hint = name,
            resolution_state = 'unresolved',
            confidence_basis_points = 2500,
            confidence_tier = 'ambiguous'
        WHERE source_scope = ?1
          AND (
              target_symbol_snapshot_id IS NOT NULL
              OR target_hint IS NULL
              OR target_hint != name
              OR resolution_state != 'unresolved'
              OR confidence_basis_points != 2500
              OR confidence_tier != 'ambiguous'
          )
        ",
        params![source_scope],
    )?;

    Ok(())
}
