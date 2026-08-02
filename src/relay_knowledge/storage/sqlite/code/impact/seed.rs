//! Symbol and module seed derivation for code-impact graph expansion.

use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::path_selection::language_suffix_for_path;

pub(super) fn symbol_seeds_for_paths(
    connection: &Connection,
    source_scope: &str,
    paths: &BTreeSet<String>,
) -> Result<ImpactSymbolSeeds, StorageError> {
    let mut path_statement = connection.prepare(
        "
        SELECT symbol_snapshot_id, path, name, qualified_name
        FROM code_repository_symbols
        WHERE source_scope = ?1 AND path = ?2
        ",
    )?;
    let mut symbol_ids = BTreeSet::new();
    let mut import_modules = BTreeSet::new();
    for path in paths {
        import_modules.extend(module_keys_for_path(path));
        let rows = path_statement.query_map(params![source_scope, path], |row| {
            Ok(ImpactSymbolRow {
                symbol_snapshot_id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                qualified_name: row.get(3)?,
            })
        })?;
        for row in rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?
        {
            symbol_ids.insert(row.symbol_snapshot_id);
            insert_non_empty(&mut import_modules, row.qualified_name);
            insert_non_empty(&mut import_modules, row.name.clone());
            for module in module_keys_for_path(&row.path) {
                import_modules.insert(format!("{module}::{}", row.name));
                import_modules.insert(format!("{module}.{}", row.name));
            }
        }
    }

    Ok(ImpactSymbolSeeds {
        symbol_ids: symbol_ids.into_iter().collect(),
        import_modules: import_modules.into_iter().collect(),
    })
}

pub(super) fn import_module_seeds(
    changed_paths: &BTreeSet<String>,
    changed_symbols: &ImpactSymbolSeeds,
    deleted_symbol_names: &[String],
) -> Vec<String> {
    let mut modules = changed_symbols
        .import_modules
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in changed_paths {
        modules.extend(module_keys_for_path(path));
    }
    for name in deleted_symbol_names {
        insert_non_empty(&mut modules, name.clone());
    }

    modules.into_iter().collect()
}

pub(super) fn module_import_matches(imported_module: &str, changed_module: &str) -> bool {
    imported_module
        .match_indices(changed_module)
        .any(|(start, value)| {
            let end = start + value.len();
            module_boundary(imported_module[..start].chars().next_back())
                && module_boundary(imported_module[end..].chars().next())
        })
}

fn module_boundary(character: Option<char>) -> bool {
    character
        .map(|value| {
            matches!(
                value,
                ':' | '.'
                    | '/'
                    | '\\'
                    | ';'
                    | ','
                    | '{'
                    | '}'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '"'
                    | '\''
                    | '`'
                    | ' '
                    | '\t'
                    | '\n'
                    | '\r'
            )
        })
        .unwrap_or(true)
}

fn module_keys_for_path(path: &str) -> BTreeSet<String> {
    let normalized = path.replace('\\', "/");
    let stem = path_without_code_extension(&normalized);
    let mut modules = BTreeSet::new();
    insert_non_empty(&mut modules, stem.replace(['/', '\\'], "::"));
    insert_non_empty(&mut modules, stem.replace(['/', '\\'], "."));
    if let Some(crate_module) = rust_crate_module_key(&stem) {
        modules.insert(crate_module);
    }

    modules
}

fn path_without_code_extension(path: &str) -> String {
    if let Some((suffix, _)) = language_suffix_for_path(path) {
        return path[..path.len().saturating_sub(suffix.len())].to_owned();
    }

    path.to_owned()
}

fn rust_crate_module_key(path_stem: &str) -> Option<String> {
    let module = path_stem.strip_prefix("src/")?;
    if matches!(module, "lib" | "main") {
        return Some("crate".to_owned());
    }
    let module = module.strip_suffix("/mod").unwrap_or(module);
    let module = module.replace(['/', '\\'], "::");
    (!module.is_empty()).then(|| format!("crate::{module}"))
}

fn insert_non_empty(values: &mut BTreeSet<String>, value: String) {
    let value = value.trim();
    if !value.is_empty() {
        values.insert(value.to_owned());
    }
}

pub(super) struct ImpactSymbolSeeds {
    pub(super) symbol_ids: Vec<String>,
    import_modules: Vec<String>,
}

struct ImpactSymbolRow {
    symbol_snapshot_id: String,
    path: String,
    name: String,
    qualified_name: String,
}
