//! Computes the set of paths whose edge data needs re-finalization after an
//! incremental clone.  Stale-reference detection uses a single SQL query that
//! finds references whose `target_symbol_snapshot_id` no longer exists in the
//! new scope's symbol table — this catches both forward staleness (a changed
//! file references a removed symbol) and reverse staleness (an unchanged file
//! referenced a symbol that was renamed/removed in a changed file).

use rusqlite::{Transaction, params};

use crate::storage::StorageError;

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
/// additionally queries the database for paths containing stale references
/// (whose `target_symbol_snapshot_id` does not exist in the new scope's
/// symbol table).  When the affected set is empty (resumable session) or
/// exceeds half the total paths, the result signals full-scope fallback.
pub(crate) fn compute(
    transaction: &Transaction<'_>,
    source_scope: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
    total_path_count: usize,
) -> Result<AffectedPaths, StorageError> {
    if changed_paths.is_empty() && deleted_paths.is_empty() {
        return Ok(AffectedPaths::full_scope());
    }

    let mut paths: Vec<String> = changed_paths
        .iter()
        .chain(deleted_paths.iter())
        .cloned()
        .collect();

    let stale_paths = load_stale_reference_paths(transaction, source_scope)?;
    paths.extend(stale_paths);

    paths.sort_unstable();
    paths.dedup();

    let total = if total_path_count > 0 {
        total_path_count
    } else {
        count_distinct_paths(transaction, source_scope)? as usize
    };

    let fallback = total == 0 || paths.len() >= total / FALLBACK_FRACTION;

    Ok(AffectedPaths {
        paths,
        fallback_to_full_scope: fallback,
    })
}

/// Selects paths that contain at least one reference whose
/// `target_symbol_snapshot_id` does not exist in the new scope's symbol
/// table.  This catches both forward and reverse staleness after clone.
fn load_stale_reference_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT DISTINCT path
        FROM code_repository_references
        WHERE source_scope = ?1
          AND target_symbol_snapshot_id IS NOT NULL
          AND target_symbol_snapshot_id NOT IN (
              SELECT symbol_snapshot_id
              FROM code_repository_symbols
              WHERE source_scope = ?1
          )
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
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
