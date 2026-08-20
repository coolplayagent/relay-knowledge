//! Computes the set of paths whose edge data needs re-finalization after an
//! incremental clone. It derives changed symbol names from bounded changed
//! paths, compares their candidate distributions, and uses indexed exact/FTS
//! lookups to find dependent references and imports. This makes unchanged
//! edges participate whenever resolution can transition between unresolved,
//! unique, preferred, and ambiguous candidates.

use std::collections::BTreeSet;

use rusqlite::{Transaction, params, params_from_iter, types::Value};

use crate::{domain::code_call_targets::call_target_name_candidates, storage::StorageError};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

/// Threshold fraction: if affected paths exceed half the total paths in the
/// scope, the per-path `IN (...)` overhead exceeds the savings from skipping
/// unaffected paths, so we fall back to full-scope finalization.
const FALLBACK_FRACTION: usize = 2;
const NAME_QUERY_BATCH_SIZE: usize = 64;
const MAX_DISCOVERED_AFFECTED_PATHS: usize = 513;

pub(crate) struct AffectedPaths {
    paths: Vec<String>,
    fallback_to_full_scope: bool,
}

struct PathDiscovery {
    paths: Vec<String>,
    saturated: bool,
}

impl AffectedPaths {
    /// Returns `true` when the caller should use the existing full-scope
    /// finalization path instead of per-path-scoped phases.
    pub(crate) fn is_full_scope(&self) -> bool {
        self.fallback_to_full_scope
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.fallback_to_full_scope && self.paths.is_empty()
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

    pub(crate) fn empty() -> Self {
        Self {
            paths: Vec::new(),
            fallback_to_full_scope: false,
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
        return Ok(AffectedPaths::empty());
    }

    let mut paths: Vec<String> = changed_paths
        .iter()
        .chain(deleted_paths.iter())
        .cloned()
        .collect();

    let changed_symbol_names = load_changed_symbol_names(
        transaction,
        source_scope,
        base_scope,
        changed_paths,
        deleted_paths,
    )?;
    let replaced_base_symbol_ids =
        load_base_symbol_ids_for_paths(transaction, base_scope, changed_paths, deleted_paths)?;
    let reference_paths = load_reference_affected_paths(
        transaction,
        source_scope,
        &changed_symbol_names,
        &replaced_base_symbol_ids,
    )?;
    paths.extend(reference_paths.paths);
    let import_paths =
        load_named_import_affected_paths(transaction, source_scope, &changed_symbol_names)?;
    paths.extend(import_paths.paths);

    paths.sort_unstable();
    paths.dedup();

    let total = count_distinct_paths(transaction, source_scope)? as usize;

    let module_file_set_changed = module_file_set_changed(
        transaction,
        source_scope,
        base_scope,
        changed_paths,
        deleted_paths,
    )?;
    let fallback = total == 0
        || paths.len() >= total / FALLBACK_FRACTION
        || paths.len() >= MAX_DISCOVERED_AFFECTED_PATHS
        || reference_paths.saturated
        || import_paths.saturated
        || module_file_set_changed;

    Ok(AffectedPaths {
        paths,
        fallback_to_full_scope: fallback,
    })
}

fn load_named_import_affected_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    changed_symbol_names: &BTreeSet<String>,
) -> Result<PathDiscovery, StorageError> {
    if changed_symbol_names.is_empty() {
        return Ok(PathDiscovery {
            paths: Vec::new(),
            saturated: false,
        });
    }
    let mut paths = BTreeSet::new();
    let mut statement = transaction.prepare(
        "SELECT imports.path, files.language_id, imports.module
         FROM code_repository_search
         JOIN code_repository_imports AS imports
           ON imports.source_scope = code_repository_search.source_scope
          AND imports.import_id = code_repository_search.record_id
         JOIN code_repository_files AS files
           ON files.source_scope = imports.source_scope AND files.path = imports.path
         WHERE code_repository_search MATCH ?2
           AND code_repository_search.source_scope = ?1
           AND code_repository_search.document_kind = 'import'
           AND instr(imports.module, ?4) > 0
         LIMIT ?3",
    )?;
    for name in changed_symbol_names {
        let mut inspected_rows = 0;
        let rows = statement.query_map(
            params![
                source_scope,
                fts_identifier_query(name),
                MAX_DISCOVERED_AFFECTED_PATHS as i64,
                name
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        for row in rows {
            inspected_rows += 1;
            let (path, language, statement) = row?;
            if super::imports::languages::symbol_dependency_names(language.as_deref(), &statement)
                .iter()
                .any(|name| changed_symbol_names.contains(name))
            {
                paths.insert(path);
                if paths.len() >= MAX_DISCOVERED_AFFECTED_PATHS {
                    return Ok(PathDiscovery {
                        paths: paths.into_iter().collect(),
                        saturated: true,
                    });
                }
            }
        }
        if inspected_rows >= MAX_DISCOVERED_AFFECTED_PATHS {
            return Ok(PathDiscovery {
                paths: paths.into_iter().collect(),
                saturated: true,
            });
        }
    }

    Ok(PathDiscovery {
        paths: paths.into_iter().collect(),
        saturated: false,
    })
}

fn load_changed_symbol_names(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
) -> Result<BTreeSet<String>, StorageError> {
    let impact_paths = changed_paths
        .iter()
        .chain(deleted_paths)
        .collect::<BTreeSet<_>>();
    if impact_paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let placeholders = std::iter::repeat_n("?", impact_paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT DISTINCT name FROM code_repository_symbols
         WHERE (source_scope = ? AND path IN ({placeholders}))
            OR (source_scope = ? AND path IN ({placeholders}))"
    );
    let mut values = Vec::with_capacity(2 + impact_paths.len() * 2);
    values.push(Value::Text(source_scope.to_owned()));
    values.extend(impact_paths.iter().map(|path| Value::Text((*path).clone())));
    values.push(Value::Text(base_scope.to_owned()));
    values.extend(impact_paths.iter().map(|path| Value::Text((*path).clone())));
    let mut statement = transaction.prepare(&sql)?;
    let candidate_names = statement
        .query_map(params_from_iter(values), |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = BTreeSet::new();
    for name in candidate_names {
        if load_symbol_distribution(transaction, base_scope, &name)?
            != load_symbol_distribution(transaction, source_scope, &name)?
        {
            changed.insert(name);
        }
    }

    Ok(changed)
}

fn load_symbol_distribution(
    transaction: &Transaction<'_>,
    source_scope: &str,
    name: &str,
) -> Result<BTreeSet<(String, String, String, i64)>, StorageError> {
    let mut statement = transaction.prepare(
        "SELECT path, kind, signature, COUNT(*)
         FROM code_repository_symbols
         WHERE source_scope = ?1 AND name = ?2
         GROUP BY path, kind, signature",
    )?;
    let rows = statement.query_map(params![source_scope, name], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

fn load_base_symbol_ids_for_paths(
    transaction: &Transaction<'_>,
    base_scope: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
) -> Result<BTreeSet<String>, StorageError> {
    let impact_paths = changed_paths
        .iter()
        .chain(deleted_paths)
        .collect::<BTreeSet<_>>();
    if impact_paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let placeholders = std::iter::repeat_n("?", impact_paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT symbol_snapshot_id FROM code_repository_symbols
         WHERE source_scope = ? AND path IN ({placeholders})"
    );
    let values = std::iter::once(Value::Text(base_scope.to_owned()))
        .chain(impact_paths.iter().map(|path| Value::Text((*path).clone())));
    let mut statement = transaction.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;

    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

/// Selects paths whose exact or call-alias candidate name changed between scopes.
fn load_reference_affected_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    changed_symbol_names: &BTreeSet<String>,
    replaced_base_symbol_ids: &BTreeSet<String>,
) -> Result<PathDiscovery, StorageError> {
    if changed_symbol_names.is_empty() && replaced_base_symbol_ids.is_empty() {
        return Ok(PathDiscovery {
            paths: Vec::new(),
            saturated: false,
        });
    }
    let mut paths = BTreeSet::new();
    let names = changed_symbol_names.iter().collect::<Vec<_>>();
    for name_batch in names.chunks(NAME_QUERY_BATCH_SIZE) {
        let placeholders = std::iter::repeat_n("?", name_batch.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT reference.path, reference.name, reference.kind
             FROM code_repository_references AS reference
             WHERE reference.source_scope = ?
               AND reference.name IN ({placeholders})"
        );
        let mut values = Vec::with_capacity(1 + name_batch.len());
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(name_batch.iter().map(|name| Value::Text((*name).clone())));
        let mut statement = transaction.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (path, name, kind) = row?;
            let name_changed = if kind == "call" {
                call_target_name_candidates(&name, &path)
                    .iter()
                    .any(|candidate| changed_symbol_names.contains(candidate))
            } else {
                changed_symbol_names.contains(&name)
            };
            if name_changed {
                paths.insert(path);
                if paths.len() >= MAX_DISCOVERED_AFFECTED_PATHS {
                    return Ok(PathDiscovery {
                        paths: paths.into_iter().collect(),
                        saturated: true,
                    });
                }
            }
        }
    }

    let replaced_ids = replaced_base_symbol_ids.iter().collect::<Vec<_>>();
    for id_batch in replaced_ids.chunks(NAME_QUERY_BATCH_SIZE) {
        let placeholders = std::iter::repeat_n("?", id_batch.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT path FROM code_repository_references
             WHERE source_scope = ? AND target_symbol_snapshot_id IN ({placeholders})"
        );
        let values = std::iter::once(Value::Text(source_scope.to_owned()))
            .chain(id_batch.iter().map(|id| Value::Text((*id).clone())));
        let mut statement = transaction.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
        for row in rows {
            paths.insert(row?);
            if paths.len() >= MAX_DISCOVERED_AFFECTED_PATHS {
                return Ok(PathDiscovery {
                    paths: paths.into_iter().collect(),
                    saturated: true,
                });
            }
        }
    }

    if changed_symbol_names.is_empty() {
        return Ok(PathDiscovery {
            paths: paths.into_iter().collect(),
            saturated: false,
        });
    }

    let mut alias_statement = transaction.prepare(
        "SELECT reference.path, reference.name
         FROM code_repository_search
         JOIN code_repository_references AS reference
           ON reference.source_scope = code_repository_search.source_scope
          AND reference.reference_id = code_repository_search.record_id
         WHERE code_repository_search MATCH ?2
           AND code_repository_search.source_scope = ?1
           AND code_repository_search.document_kind = 'reference'
           AND reference.kind = 'call'
           AND instr(reference.name, ?4) > 0
         LIMIT ?3",
    )?;
    for name in changed_symbol_names {
        let mut inspected_rows = 0;
        let rows = alias_statement.query_map(
            params![
                source_scope,
                fts_identifier_query(name),
                MAX_DISCOVERED_AFFECTED_PATHS as i64,
                name
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        for row in rows {
            inspected_rows += 1;
            let (path, reference_name) = row?;
            if call_target_name_candidates(&reference_name, &path)
                .iter()
                .any(|candidate| changed_symbol_names.contains(candidate))
            {
                paths.insert(path);
                if paths.len() >= MAX_DISCOVERED_AFFECTED_PATHS {
                    return Ok(PathDiscovery {
                        paths: paths.into_iter().collect(),
                        saturated: true,
                    });
                }
            }
        }
        if inspected_rows >= MAX_DISCOVERED_AFFECTED_PATHS {
            return Ok(PathDiscovery {
                paths: paths.into_iter().collect(),
                saturated: true,
            });
        }
    }

    Ok(PathDiscovery {
        paths: paths.into_iter().collect(),
        saturated: false,
    })
}

fn fts_identifier_query(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn module_file_set_changed(
    transaction: &Transaction<'_>,
    source_scope: &str,
    base_scope: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
) -> Result<bool, StorageError> {
    let impact_paths = changed_paths
        .iter()
        .chain(deleted_paths)
        .collect::<BTreeSet<_>>();
    if impact_paths.is_empty() {
        return Ok(false);
    }
    let placeholders = std::iter::repeat_n("?", impact_paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT EXISTS (
            SELECT path FROM code_repository_files
            WHERE source_scope = ? AND path IN ({placeholders})
            EXCEPT
            SELECT path FROM code_repository_files
            WHERE source_scope = ? AND path IN ({placeholders})
        ) OR EXISTS (
            SELECT path FROM code_repository_files
            WHERE source_scope = ? AND path IN ({placeholders})
            EXCEPT
            SELECT path FROM code_repository_files
            WHERE source_scope = ? AND path IN ({placeholders})
        )"
    );
    let mut values = Vec::with_capacity(4 + impact_paths.len() * 4);
    for scope in [source_scope, base_scope, base_scope, source_scope] {
        values.push(Value::Text(scope.to_owned()));
        values.extend(impact_paths.iter().map(|path| Value::Text((*path).clone())));
    }
    transaction
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
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
