//! Normalizes and resolves ordinary code references during finalization.

use rusqlite::{Transaction, params, params_from_iter, types::Value};

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

/// Path-scoped variant of [`normalize_unresolved`]: only nulls
/// `target_symbol_snapshot_id` on references whose `path` is in
/// `affected_paths`, preserving valid resolutions on unchanged paths.
pub(super) fn normalize_unresolved_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<(), StorageError> {
    let mut paths = affected_paths.to_vec();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }
    for path_chunk in paths.chunks(500) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(path_chunk.len() + 1);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        transaction.execute(
            &format!(
                "
                UPDATE code_repository_references
                SET target_symbol_snapshot_id = NULL,
                    target_hint = name,
                    resolution_state = 'unresolved',
                    confidence_basis_points = 2500,
                    confidence_tier = 'ambiguous'
                WHERE source_scope = ?
                  AND path IN ({placeholders})
                  AND (
                      target_symbol_snapshot_id IS NOT NULL
                      OR target_hint IS NULL
                      OR target_hint != name
                      OR resolution_state != 'unresolved'
                      OR confidence_basis_points != 2500
                      OR confidence_tier != 'ambiguous'
                  )
                "
            ),
            params_from_iter(values),
        )?;
    }

    Ok(())
}

/// Path-scoped variant of [`resolve`]: only re-resolves references whose
/// `path` is in `affected_paths`.  The `unique_symbol` / `symbol_names`
/// CTEs consider ALL symbols in the scope (no path filter) because a
/// reference in an affected path may resolve to a symbol in an unchanged
/// path.
pub(super) fn resolve_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<(), StorageError> {
    let mut paths = affected_paths.to_vec();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }
    for path_chunk in paths.chunks(400) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        // Step 1: unique name match
        let mut values = Vec::with_capacity(path_chunk.len() * 2 + 3);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        values.push(Value::Text(source_scope.to_owned()));
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        transaction.execute(
            &format!(
                "
                WITH reference_names AS (
                    SELECT DISTINCT name
                    FROM code_repository_references
                    WHERE source_scope = ? AND kind != 'call'
                      AND path IN ({placeholders})
                ),
                unique_symbol AS (
                    SELECT name, MIN(symbol_snapshot_id) AS symbol_snapshot_id
                    FROM code_repository_symbols
                    WHERE source_scope = ?
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
                WHERE reference.source_scope = ?
                  AND reference.kind != 'call'
                  AND reference.path IN ({placeholders})
                  AND reference.name = unique_symbol.name
                "
            ),
            params_from_iter(values),
        )?;
        // Step 2: name + path match
        let mut values = Vec::with_capacity(path_chunk.len() * 2 + 3);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        values.push(Value::Text(source_scope.to_owned()));
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        transaction.execute(
            &format!(
                "
                WITH reference_pairs AS (
                    SELECT DISTINCT name, path
                    FROM code_repository_references
                    WHERE source_scope = ? AND kind != 'call'
                      AND resolution_state != 'resolved'
                      AND path IN ({placeholders})
                ),
                unique_path_symbol AS (
                    SELECT name, path, MIN(symbol_snapshot_id) AS symbol_snapshot_id
                    FROM code_repository_symbols
                    WHERE source_scope = ?
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
                WHERE reference.source_scope = ?
                  AND reference.kind != 'call'
                  AND reference.resolution_state != 'resolved'
                  AND reference.path IN ({placeholders})
                  AND reference.name = unique_path_symbol.name
                  AND reference.path = unique_path_symbol.path
                "
            ),
            params_from_iter(values),
        )?;
        // Step 3: ambiguous marking
        let mut values = Vec::with_capacity(path_chunk.len() * 2 + 3);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        values.push(Value::Text(source_scope.to_owned()));
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        transaction.execute(
            &format!(
                "
                WITH reference_names AS (
                    SELECT DISTINCT name
                    FROM code_repository_references
                    WHERE source_scope = ? AND kind != 'call'
                      AND resolution_state = 'unresolved'
                      AND path IN ({placeholders})
                ),
                symbol_names AS (
                    SELECT DISTINCT name
                    FROM code_repository_symbols
                    WHERE source_scope = ?
                      AND name IN (SELECT name FROM reference_names)
                )
                UPDATE code_repository_references AS reference
                SET resolution_state = 'ambiguous',
                    confidence_basis_points = 5000,
                    confidence_tier = 'ambiguous'
                FROM symbol_names
                WHERE reference.source_scope = ?
                  AND reference.kind != 'call'
                  AND reference.resolution_state = 'unresolved'
                  AND reference.path IN ({placeholders})
                  AND reference.name = symbol_names.name
                "
            ),
            params_from_iter(values),
        )?;
    }

    Ok(())
}
