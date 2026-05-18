use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use super::{SymbolKey, load_symbol_keys_once, path_matches_candidate, typescript_imports};
use crate::storage::StorageError;

pub(super) fn resolve_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
    symbol_cache: &mut Option<Vec<SymbolKey>>,
) -> Result<(), StorageError> {
    let imports = load_resolved_typescript_imports(transaction, source_scope)?;
    if imports.is_empty() {
        return Ok(());
    }

    let symbols = load_symbol_keys_once(transaction, source_scope, symbol_cache)?;
    let symbols_by_name = symbols_by_name(symbols);
    let mut update_reference = transaction.prepare(
        "
        UPDATE code_repository_references
        SET target_symbol_snapshot_id = ?4,
            target_hint = ?5,
            resolution_state = 'resolved',
            confidence_basis_points = 8500,
            confidence_tier = 'inferred'
        WHERE source_scope = ?1
          AND path = ?2
          AND name = ?3
          AND resolution_state != 'resolved'
        ",
    )?;

    for import in imports {
        for binding in typescript_imports::named_import_bindings(&import.module) {
            let Some(symbol) = unique_imported_symbol(
                &symbols_by_name,
                &import.target_hint,
                &binding.imported_name,
            ) else {
                continue;
            };
            update_reference.execute(params![
                source_scope,
                import.path.as_str(),
                binding.local_name.as_str(),
                symbol.symbol_snapshot_id.as_str(),
                symbol.name.as_str(),
            ])?;
        }
    }

    Ok(())
}

struct ResolvedImport {
    path: String,
    module: String,
    target_hint: String,
}

fn load_resolved_typescript_imports(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<ResolvedImport>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT import_row.path, import_row.module, import_row.target_hint
        FROM code_repository_imports import_row
        INNER JOIN code_repository_files file
            ON file.source_scope = import_row.source_scope AND file.path = import_row.path
        WHERE import_row.source_scope = ?1
          AND import_row.resolution_state = 'resolved'
          AND import_row.target_hint IS NOT NULL
          AND file.language_id IN ('typescript', 'tsx')
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ResolvedImport {
            path: row.get(0)?,
            module: row.get(1)?,
            target_hint: row.get(2)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn symbols_by_name(symbols: &[SymbolKey]) -> BTreeMap<&str, Vec<&SymbolKey>> {
    let mut symbols_by_name = BTreeMap::<&str, Vec<&SymbolKey>>::new();
    for symbol in symbols {
        symbols_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }

    symbols_by_name
}

fn unique_imported_symbol<'a>(
    symbols_by_name: &BTreeMap<&'a str, Vec<&'a SymbolKey>>,
    target_hint: &str,
    imported_name: &str,
) -> Option<&'a SymbolKey> {
    let mut matches = symbols_by_name
        .get(imported_name)?
        .iter()
        .copied()
        .filter(|symbol| path_matches_candidate(&symbol.path, target_hint));
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}
