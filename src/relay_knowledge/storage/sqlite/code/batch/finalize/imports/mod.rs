//! Coordinates language-aware import finalization through bounded path and symbol owners.

use std::collections::BTreeMap;

use rusqlite::{Transaction, params, params_from_iter, types::Value};

use super::{
    search_documents,
    symbols::{self, SymbolKey},
};
use crate::storage::StorageError;

mod languages;
mod module_paths;
mod specifier;
mod symbol_targets;

pub(super) use languages::typescript;

pub(super) fn resolve(
    transaction: &Transaction<'_>,
    source_scope: &str,
    files: &BTreeMap<String, String>,
    symbol_cache: &mut Option<Vec<SymbolKey>>,
) -> Result<(), StorageError> {
    let module_paths = module_paths::index(files);
    let imports = load_import_keys(transaction, source_scope)?;
    let symbols_by_name = if imports
        .iter()
        .any(|import| requires_symbol_index(import, files))
    {
        let mut symbols_by_name = BTreeMap::<String, Vec<SymbolKey>>::new();
        for symbol in symbols::load_once(transaction, source_scope, symbol_cache)? {
            symbols_by_name
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol.clone());
        }
        symbols_by_name
    } else {
        BTreeMap::new()
    };
    let mut update_import = transaction.prepare(
        "
        UPDATE code_repository_imports
        SET target_hint = ?3,
            resolution_state = ?4,
            confidence_basis_points = ?5,
            confidence_tier = ?6
        WHERE source_scope = ?1 AND import_id = ?2
        ",
    )?;
    for import in imports {
        let resolution = languages::resolve(
            files.get(&import.path).map(String::as_str),
            &import.path,
            &import.module,
            &module_paths,
            &symbols_by_name,
        );
        let (state, confidence, tier, target_hint) = resolution_fields(resolution, &import.module);
        update_import.execute(params![
            source_scope,
            import.import_id,
            target_hint,
            state,
            confidence,
            tier
        ])?;
    }

    search_documents::rebuild_import_search_documents(transaction, source_scope)
}

/// Path-scoped variant of [`resolve`]: only re-resolves imports whose `path`
/// is in `affected_paths`.  The symbol index still loads ALL symbols in the
/// scope because an import in an affected path may resolve to a symbol in an
/// unchanged path.
pub(super) fn resolve_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    files: &BTreeMap<String, String>,
    affected_paths: &[&str],
    symbol_cache: &mut Option<Vec<SymbolKey>>,
) -> Result<(), StorageError> {
    let module_paths = module_paths::index(files);
    let imports = load_import_keys_for_paths(transaction, source_scope, affected_paths)?;
    let symbols_by_name = if imports
        .iter()
        .any(|import| requires_symbol_index(import, files))
    {
        let mut symbols_by_name = BTreeMap::<String, Vec<SymbolKey>>::new();
        for symbol in symbols::load_once(transaction, source_scope, symbol_cache)? {
            symbols_by_name
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol.clone());
        }
        symbols_by_name
    } else {
        BTreeMap::new()
    };
    let mut update_import = transaction.prepare(
        "
        UPDATE code_repository_imports
        SET target_hint = ?3,
            resolution_state = ?4,
            confidence_basis_points = ?5,
            confidence_tier = ?6
        WHERE source_scope = ?1 AND import_id = ?2
        ",
    )?;
    for import in imports {
        let resolution = languages::resolve(
            files.get(&import.path).map(String::as_str),
            &import.path,
            &import.module,
            &module_paths,
            &symbols_by_name,
        );
        let (state, confidence, tier, target_hint) = resolution_fields(resolution, &import.module);
        update_import.execute(params![
            source_scope,
            import.import_id,
            target_hint,
            state,
            confidence,
            tier
        ])?;
    }

    search_documents::rebuild_import_search_documents_for_paths(
        transaction,
        source_scope,
        affected_paths,
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ImportResolution {
    Resolved(String),
    Ambiguous,
    Unresolved,
}

#[derive(Debug)]
struct ImportKey {
    import_id: String,
    path: String,
    module: String,
}

fn requires_symbol_index(import: &ImportKey, files: &BTreeMap<String, String>) -> bool {
    let statement = import.module.trim();
    match files.get(&import.path).map(String::as_str) {
        Some("python") => statement.starts_with("from "),
        Some("java") => statement
            .trim_end_matches(';')
            .strip_prefix("import ")
            .is_some_and(|body| body.trim_start().starts_with("static ")),
        Some("typescript" | "tsx") => {
            languages::typescript::needs_symbol_index(&import.path, statement)
        }
        _ => false,
    }
}

fn load_import_keys(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<ImportKey>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT import_id, path, module
        FROM code_repository_imports
        WHERE source_scope = ?1
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ImportKey {
            import_id: row.get(0)?,
            path: row.get(1)?,
            module: row.get(2)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn load_import_keys_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<Vec<ImportKey>, StorageError> {
    let mut paths = affected_paths.to_vec();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut all_imports = Vec::new();
    for path_chunk in paths.chunks(500) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(path_chunk.len() + 1);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        let mut statement = transaction.prepare(&format!(
            "
                SELECT import_id, path, module
                FROM code_repository_imports
                WHERE source_scope = ? AND path IN ({placeholders})
                "
        ))?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok(ImportKey {
                import_id: row.get(0)?,
                path: row.get(1)?,
                module: row.get(2)?,
            })
        })?;
        all_imports.extend(rows.collect::<Result<Vec<_>, _>>()?);
    }

    Ok(all_imports)
}

fn resolution_fields(
    resolution: ImportResolution,
    module: &str,
) -> (&'static str, u16, &'static str, String) {
    match resolution {
        ImportResolution::Resolved(target_hint) => ("resolved", 8_000, "inferred", target_hint),
        ImportResolution::Ambiguous => ("ambiguous", 5_000, "ambiguous", module.to_owned()),
        ImportResolution::Unresolved => ("unresolved", 2_500, "ambiguous", module.to_owned()),
    }
}
