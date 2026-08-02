use std::collections::BTreeMap;

use super::super::{ImportResolution, module_paths, specifier::quoted};

#[cfg(test)]
#[path = "c_tests.rs"]
mod tests;

pub(super) fn resolve(
    import_path: &str,
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let Some((target, is_quoted)) = include_target(statement) else {
        return ImportResolution::Unresolved;
    };
    let mut candidates = Vec::new();
    if is_quoted
        && let Some(relative) =
            module_paths::normalize_join(module_paths::parent(import_path), target)
    {
        module_paths::push_unique(&mut candidates, relative);
    }
    module_paths::push_unique(&mut candidates, target.to_owned());
    if !target.starts_with("include/") {
        module_paths::push_unique(&mut candidates, format!("include/{target}"));
    }

    module_paths::resolve_first_file(&candidates, is_quoted, indexed_module_paths)
}

fn include_target(statement: &str) -> Option<(&str, bool)> {
    let statement = statement.trim();
    if !statement.starts_with("#include") {
        return None;
    }
    if let Some(target) = quoted(statement) {
        return Some((target, true));
    }
    let start = statement.find('<')?;
    let rest = &statement[start + 1..];
    let end = rest.find('>')?;

    Some((&rest[..end], false))
}
