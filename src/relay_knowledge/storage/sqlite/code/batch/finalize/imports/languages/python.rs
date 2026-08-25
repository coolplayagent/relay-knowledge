use std::collections::BTreeMap;

use super::super::{super::symbols::SymbolKey, ImportResolution, module_paths, symbol_targets};
use crate::code::source_roots::source_relative_path;

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;

pub(super) fn resolve(
    import_path: &str,
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    if !(import_path.ends_with(".py") || import_path.ends_with(".pyw")) {
        return ImportResolution::Unresolved;
    }
    let statement = statement.trim().trim_end_matches(';').trim();
    if let Some(body) = statement.strip_prefix("from ") {
        let Some((module, names)) = body.split_once(" import ") else {
            return ImportResolution::Unresolved;
        };
        let module_paths = module_path_candidates(import_path, module.trim());
        if module_paths.is_empty() {
            return ImportResolution::Unresolved;
        }
        let imported_names = parse_imported_names(names);
        return combined_resolution(
            imported_names.iter().map(|name| {
                resolve_imported_name(
                    name,
                    module_paths.as_slice(),
                    indexed_module_paths,
                    symbols_by_name,
                )
            }),
            statement,
        );
    }
    if let Some(body) = statement.strip_prefix("import ") {
        let resolved = body
            .split(',')
            .filter_map(|part| {
                let module = part
                    .trim()
                    .split_once(" as ")
                    .map_or(part.trim(), |(module, _)| module.trim());
                absolute_module_path(module)
            })
            .any(|module_path| module_exists(&module_path, indexed_module_paths));
        return if resolved {
            ImportResolution::Resolved(statement.to_owned())
        } else {
            ImportResolution::Unresolved
        };
    }

    ImportResolution::Unresolved
}

fn resolve_imported_name(
    name: &str,
    module_paths: &[String],
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let symbol_paths = module_paths
        .iter()
        .flat_map(|module_path| module_files(module_path))
        .collect::<Vec<_>>();
    match symbol_targets::resolve_name_in_paths(name, &symbol_paths, symbols_by_name) {
        ImportResolution::Unresolved => {
            if module_paths.iter().any(|module_path| {
                module_exists(&format!("{module_path}/{name}"), indexed_module_paths)
            }) {
                ImportResolution::Resolved(name.to_owned())
            } else {
                ImportResolution::Unresolved
            }
        }
        resolution => resolution,
    }
}

fn combined_resolution(
    results: impl IntoIterator<Item = ImportResolution>,
    statement: &str,
) -> ImportResolution {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut ambiguous = false;
    for result in results {
        total += 1;
        match result {
            ImportResolution::Resolved(_) => resolved += 1,
            ImportResolution::Ambiguous => ambiguous = true,
            ImportResolution::Unresolved => {}
        }
    }
    if total == 0 {
        return ImportResolution::Unresolved;
    }
    if ambiguous || (resolved > 0 && resolved < total) {
        return ImportResolution::Ambiguous;
    }
    if resolved == total {
        return ImportResolution::Resolved(statement.to_owned());
    }

    ImportResolution::Unresolved
}

fn module_exists(module_path: &str, indexed_module_paths: &BTreeMap<String, Vec<String>>) -> bool {
    module_files(module_path)
        .iter()
        .any(|file_path| indexed_module_paths.contains_key(&module_paths::normalize(file_path)))
}

fn module_files(module_path: &str) -> Vec<String> {
    vec![
        format!("{module_path}.py"),
        format!("{module_path}.pyw"),
        format!("{module_path}/__init__.py"),
    ]
}

fn absolute_module_path(module: &str) -> Option<String> {
    let module = module.trim();
    (!module.is_empty() && !module.starts_with('.')).then(|| module.replace('.', "/"))
}

fn module_path_candidates(import_path: &str, module: &str) -> Vec<String> {
    let module = module.trim();
    if module.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if module.starts_with('.') {
        if let Some(relative) = relative_module_path(import_path, module) {
            candidates.push(relative);
        }
    } else if let Some(absolute) = absolute_module_path(module) {
        candidates.push(absolute);
    }
    candidates.sort();
    candidates.dedup();

    candidates
}

fn relative_module_path(import_path: &str, module: &str) -> Option<String> {
    let dot_count = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = module[dot_count..].replace('.', "/");
    let import_path = source_relative_path(import_path);
    let mut package = module_paths::parent(&import_path)
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let drop_count = dot_count.saturating_sub(1);
    if drop_count > package.len() {
        return None;
    }
    for _ in 0..drop_count {
        package.pop();
    }
    let base = package.join("/");
    if remainder.is_empty() {
        return (!base.is_empty()).then_some(base);
    }

    module_paths::normalize_join(&base, &remainder)
}

fn parse_imported_names(names: &str) -> Vec<String> {
    names
        .replace(['(', ')', '\\'], " ")
        .split(',')
        .filter_map(|part| {
            let name = part
                .trim()
                .split_once(" as ")
                .map_or(part.trim(), |(name, _)| name.trim());
            let name = name.trim_start_matches('.');
            (!name.is_empty() && name != "*").then(|| name.to_owned())
        })
        .collect()
}
