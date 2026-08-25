use std::collections::BTreeMap;

use super::{super::symbols::SymbolKey, ImportResolution};

mod c;
mod go;
mod java;
mod python;
pub(in super::super) mod typescript;

pub(super) fn resolve(
    language: Option<&str>,
    import_path: &str,
    statement: &str,
    module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    match language {
        Some("c" | "cpp") => c::resolve(import_path, statement, module_paths),
        Some("python") => python::resolve(import_path, statement, module_paths, symbols_by_name),
        Some("go") => go::resolve(statement, module_paths),
        Some("java") => java::resolve(statement, module_paths, symbols_by_name),
        Some("typescript" | "tsx") => {
            typescript::resolve_import(import_path, statement, module_paths, symbols_by_name)
        }
        _ => ImportResolution::Unresolved,
    }
}
