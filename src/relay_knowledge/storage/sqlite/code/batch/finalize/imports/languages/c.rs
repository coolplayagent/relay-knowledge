use std::collections::BTreeMap;

use super::super::{
    ImportResolution, module_paths,
    specifier::{CIncludeDelimiter, c_include_specifier},
};

#[cfg(test)]
#[path = "c_tests.rs"]
mod tests;

pub(super) fn resolve(
    import_path: &str,
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let Some(specifier) = c_include_specifier(statement) else {
        return ImportResolution::Unresolved;
    };
    let target = specifier.target;
    let is_quoted = specifier.delimiter == CIncludeDelimiter::Quoted;
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
