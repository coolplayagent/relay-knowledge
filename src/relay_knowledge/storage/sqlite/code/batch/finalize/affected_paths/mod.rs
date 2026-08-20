//! Computes the set of paths whose edge data needs re-finalization after an
//! incremental clone. It finds references whose target no longer exists or
//! whose candidate name/cardinality/callable metadata differs from the base
//! scope. This makes unchanged references participate whenever resolution can
//! transition between unique, preferred, and ambiguous candidates.

use std::collections::BTreeSet;

use rusqlite::{Transaction, params};

use crate::{domain::code_call_targets::call_target_name_candidates, storage::StorageError};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

/// Threshold fraction: if affected paths exceed half the total paths in the
/// scope, the per-path `IN (...)` overhead exceeds the savings from skipping
/// unaffected paths, so we fall back to full-scope finalization.
const FALLBACK_FRACTION: usize = 2;

pub(crate) struct AffectedPaths {
    paths: Vec<String>,
    fallback_to_full_scope: bool,
}

impl AffectedPaths {
    /// Returns `true` when the caller should use the existing full-scope
    /// finalization path instead of per-path-scoped phases.
    pub(crate) fn is_full_scope(&self) -> bool {
        self.fallback_to_full_scope
    }

    /// Convenience accessor returning `&[&str]` for SQL parameter binding.
    pub(crate) fn path_refs(&self) -> Vec<&str> {
        self.paths.iter().map(String::as_str).collect()
    }

    /// Constructs an instance that signals full-scope finalization.
    pub(crate) fn full_scope() -> Self {
        Self {
            paths: Vec::new(),
            fallback_to_full_scope: true,
        }
    }
}

/// Computes the affected-path set for an incremental session.
///
/// `changed_paths` and `deleted_paths` come from the session.  The function
/// additionally queries the database for paths containing stale references or
/// references to names whose symbol count or callable shape changed from
/// `base_scope`. A module/file-set change promotes all edge finalization to the
/// full scope so import-driven downstream projections stay consistent. When
/// the affected set is empty (resumable session) or exceeds half the total
/// paths in the scope, the result also signals full-scope fallback.
pub(crate) fn compute(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
) -> Result<AffectedPaths, StorageError> {
    if changed_paths.is_empty() && deleted_paths.is_empty() {
        return Ok(AffectedPaths::full_scope());
    }

    let mut paths: Vec<String> = changed_paths
        .iter()
        .chain(deleted_paths.iter())
        .cloned()
        .collect();

    let changed_symbol_names = load_changed_symbol_names(transaction, source_scope, base_scope)?;
    let reference_paths =
        load_reference_affected_paths(transaction, source_scope, &changed_symbol_names)?;
    paths.extend(reference_paths);

    paths.sort_unstable();
    paths.dedup();

    let total = count_distinct_paths(transaction, source_scope)? as usize;

    let module_file_set_changed = module_file_set_changed(transaction, source_scope, base_scope)?;
    let fallback =
        total == 0 || paths.len() >= total / FALLBACK_FRACTION || module_file_set_changed;

    Ok(AffectedPaths {
        paths,
        fallback_to_full_scope: fallback,
    })
}

fn load_changed_symbol_names(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        WITH base_symbol_counts AS (
            SELECT name, COUNT(*) AS symbol_count
            FROM code_repository_symbols
            WHERE source_scope = ?2
            GROUP BY name
        ),
        current_symbol_counts AS (
            SELECT name, COUNT(*) AS symbol_count
            FROM code_repository_symbols
            WHERE source_scope = ?1
            GROUP BY name
        )
        SELECT base.name
        FROM base_symbol_counts base
        LEFT JOIN current_symbol_counts current ON current.name = base.name
        WHERE current.name IS NULL OR current.symbol_count != base.symbol_count
        UNION
        SELECT current.name
        FROM current_symbol_counts current
        LEFT JOIN base_symbol_counts base ON base.name = current.name
        WHERE base.name IS NULL
        UNION
        SELECT base.name
        FROM (
            SELECT name, kind, signature, COUNT(*) AS shape_count
            FROM code_repository_symbols
            WHERE source_scope = ?2
            GROUP BY name, kind, signature
        ) base
        LEFT JOIN (
            SELECT name, kind, signature, COUNT(*) AS shape_count
            FROM code_repository_symbols
            WHERE source_scope = ?1
            GROUP BY name, kind, signature
        ) current
          ON current.name = base.name
         AND current.kind = base.kind
         AND current.signature = base.signature
        WHERE current.name IS NULL OR current.shape_count != base.shape_count
        UNION
        SELECT current.name
        FROM (
            SELECT name, kind, signature, COUNT(*) AS shape_count
            FROM code_repository_symbols
            WHERE source_scope = ?1
            GROUP BY name, kind, signature
        ) current
        LEFT JOIN (
            SELECT name, kind, signature, COUNT(*) AS shape_count
            FROM code_repository_symbols
            WHERE source_scope = ?2
            GROUP BY name, kind, signature
        ) base
          ON base.name = current.name
         AND base.kind = current.kind
         AND base.signature = current.signature
        WHERE base.name IS NULL OR base.shape_count != current.shape_count
        ",
    )?;
    let rows = statement.query_map(params![source_scope, base_scope], |row| row.get(0))?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

/// Selects paths with a missing target or a candidate name whose symbol
/// cardinality or callable metadata changed between scopes.
fn load_reference_affected_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    changed_symbol_names: &BTreeSet<String>,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT reference.path, reference.name, reference.kind,
               reference.target_symbol_snapshot_id IS NOT NULL
                   AND target.symbol_snapshot_id IS NULL AS target_missing
        FROM code_repository_references reference
        LEFT JOIN code_repository_symbols target
          ON target.source_scope = reference.source_scope
         AND target.symbol_snapshot_id = reference.target_symbol_snapshot_id
        WHERE reference.source_scope = ?1
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, bool>(3)?,
        ))
    })?;
    let mut paths = Vec::new();
    for row in rows {
        let (path, name, kind, target_missing) = row?;
        let name_changed = if kind == "call" {
            call_target_name_candidates(&name, &path)
                .iter()
                .any(|candidate| changed_symbol_names.contains(candidate))
        } else {
            changed_symbol_names.contains(&name)
        };
        if target_missing || name_changed {
            paths.push(path);
        }
    }

    Ok(paths)
}

fn module_file_set_changed(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "
            SELECT EXISTS (
                SELECT path FROM code_repository_files WHERE source_scope = ?1
                EXCEPT
                SELECT path FROM code_repository_files WHERE source_scope = ?2
            ) OR EXISTS (
                SELECT path FROM code_repository_files WHERE source_scope = ?2
                EXCEPT
                SELECT path FROM code_repository_files WHERE source_scope = ?1
            )
            ",
            params![source_scope, base_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn count_distinct_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<i64, StorageError> {
    transaction
        .query_row(
            "SELECT COUNT(DISTINCT path) FROM code_repository_files WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}
