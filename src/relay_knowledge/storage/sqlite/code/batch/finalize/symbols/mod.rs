//! Loads and matches the shared scope-local symbol catalog for finalization.

use rusqlite::{Transaction, params};

use crate::{
    code::source_roots::source_module_candidates, domain::RepositoryCodeRange,
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub(super) struct SymbolKey {
    pub(super) symbol_snapshot_id: String,
    pub(super) path: String,
    pub(super) name: String,
    pub(super) line_range: RepositoryCodeRange,
}

pub(super) fn load_once<'a>(
    transaction: &Transaction<'_>,
    source_scope: &str,
    symbol_cache: &'a mut Option<Vec<SymbolKey>>,
) -> Result<&'a [SymbolKey], StorageError> {
    if symbol_cache.is_none() {
        *symbol_cache = Some(load(transaction, source_scope)?);
    }

    Ok(symbol_cache
        .as_deref()
        .expect("symbol cache should be initialized after load"))
}

pub(super) fn path_matches_candidate(path: &str, candidate: &str) -> bool {
    let candidate = normalize_module_path(candidate);
    path == candidate.as_str()
        || source_module_candidates(path)
            .iter()
            .any(|module_path| module_path == &candidate)
}

fn load(transaction: &Transaction<'_>, source_scope: &str) -> Result<Vec<SymbolKey>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT symbol_snapshot_id, path, name, line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC, line_end DESC, name ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(SymbolKey {
            symbol_snapshot_id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            line_range: RepositoryCodeRange {
                start: row.get(3)?,
                end: row.get(4)?,
            },
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn normalize_module_path(path: &str) -> String {
    let mut normalized = path.trim();
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped;
    }
    normalized.trim_end_matches('/').to_owned()
}
