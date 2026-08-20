//! Computes the set of paths whose edge data needs re-finalization after an
//! incremental clone. A single SQL query finds references whose target no
//! longer exists or whose symbol-name cardinality differs from the base scope.
//! The latter makes unchanged references participate when a name transitions
//! between unique and ambiguous resolution.

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
    finalize_imports_for_full_scope: bool,
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

    /// Import resolution depends on the complete module/file set, including
    /// side-effect imports that have no corresponding reference row.
    pub(crate) fn imports_need_full_scope(&self) -> bool {
        self.finalize_imports_for_full_scope
    }

    /// Constructs an instance that signals full-scope finalization.
    pub(crate) fn full_scope() -> Self {
        Self {
            paths: Vec::new(),
            fallback_to_full_scope: true,
            finalize_imports_for_full_scope: true,
        }
    }
}

/// Computes the affected-path set for an incremental session.
///
/// `changed_paths` and `deleted_paths` come from the session.  The function
/// additionally queries the database for paths containing stale references or
/// references to names whose symbol count changed from `base_scope`. When the
/// affected set is empty (resumable session) or exceeds half the total paths
/// in the scope, the result signals full-scope fallback.
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
    let reference_paths = load_reference_affected_paths(
        transaction,
        source_scope,
        base_scope,
        &changed_symbol_names,
    )?;
    paths.extend(reference_paths);

    paths.sort_unstable();
    paths.dedup();

    let total = count_distinct_paths(transaction, source_scope)? as usize;

    let fallback = total == 0 || paths.len() >= total / FALLBACK_FRACTION;

    Ok(AffectedPaths {
        paths,
        fallback_to_full_scope: fallback,
        finalize_imports_for_full_scope: module_file_set_changed(
            transaction,
            source_scope,
            base_scope,
        )?,
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
        ",
    )?;
    let rows = statement.query_map(params![source_scope, base_scope], |row| row.get(0))?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

/// Selects paths with a missing target or a name whose symbol cardinality
/// changed between the persisted base and the new incremental scope.
fn load_reference_affected_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
    changed_symbol_names: &BTreeSet<String>,
) -> Result<Vec<String>, StorageError> {
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
        ),
        changed_symbol_names AS (
            SELECT base.name
            FROM base_symbol_counts base
            LEFT JOIN current_symbol_counts current ON current.name = base.name
            WHERE current.name IS NULL OR current.symbol_count != base.symbol_count
            UNION
            SELECT current.name
            FROM current_symbol_counts current
            LEFT JOIN base_symbol_counts base ON base.name = current.name
            WHERE base.name IS NULL
        )
        SELECT DISTINCT reference.path
        FROM code_repository_references reference
        LEFT JOIN code_repository_symbols target
          ON target.source_scope = reference.source_scope
         AND target.symbol_snapshot_id = reference.target_symbol_snapshot_id
        LEFT JOIN changed_symbol_names changed_name ON changed_name.name = reference.name
        WHERE reference.source_scope = ?1
          AND (
              (reference.target_symbol_snapshot_id IS NOT NULL
               AND target.symbol_snapshot_id IS NULL)
              OR changed_name.name IS NOT NULL
          )
        ",
    )?;
    let rows = statement.query_map(params![source_scope, base_scope], |row| {
        row.get::<_, String>(0)
    })?;
    let mut paths = rows.collect::<Result<Vec<_>, _>>()?;

    let mut aliases = transaction.prepare(
        "
        SELECT path, name
        FROM code_repository_references
        WHERE source_scope = ?1 AND kind = 'call'
        ",
    )?;
    let rows = aliases.query_map(params![source_scope], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (path, name) = row?;
        if call_target_name_candidates(&name, &path)
            .iter()
            .skip(1)
            .any(|candidate| changed_symbol_names.contains(candidate))
        {
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
