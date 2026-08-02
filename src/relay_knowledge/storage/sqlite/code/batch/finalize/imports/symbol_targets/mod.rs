//! Resolves imported symbol names against bounded target-path candidates.

use std::collections::BTreeMap;

use super::{super::symbols, ImportResolution};
use crate::storage::sqlite::code::batch::finalize::symbols::SymbolKey;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn resolve_name_in_paths(
    name: &str,
    symbol_paths: &[String],
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let matching_symbols = symbols_by_name.get(name).map_or(0, |candidates| {
        candidates
            .iter()
            .filter(|symbol| {
                symbol_paths
                    .iter()
                    .any(|module_path| symbols::path_matches_candidate(&symbol.path, module_path))
            })
            .take(2)
            .count()
    });
    match matching_symbols {
        1 => ImportResolution::Resolved(name.to_owned()),
        2.. => ImportResolution::Ambiguous,
        _ => ImportResolution::Unresolved,
    }
}
