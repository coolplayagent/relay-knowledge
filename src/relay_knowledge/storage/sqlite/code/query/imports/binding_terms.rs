//! Named-import binding parsing and usage-identifier term extraction.

use super::super::identifiers::identifier_terms_equivalent;

pub(super) fn named_import_binding_count_for_query(module: &str, query: &str) -> Option<usize> {
    let (start, end) = named_import_bounds(module)?;
    let query_terms = query_terms(query);
    let mut binding_count = 0;
    let mut query_is_bound = false;
    for binding in module[start + 1..end].split(',') {
        let binding = binding.trim().trim_start_matches("type ").trim();
        if binding.is_empty() {
            continue;
        }
        binding_count += 1;
        if import_binding_name(binding)
            .is_some_and(|binding_name| import_binding_matches_query(binding_name, &query_terms))
        {
            query_is_bound = true;
        }
    }

    query_is_bound.then_some(binding_count)
}

pub(super) fn named_import_binding_terms(module: &str) -> Vec<String> {
    let Some((start, end)) = named_import_bounds(module) else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    for binding in module[start + 1..end].split(',') {
        let Some(binding_names) = import_binding_names(binding) else {
            continue;
        };
        for term in import_usage_identifier_terms(binding_names.local) {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }

    terms
}

pub(super) fn named_import_binding_terms_for_query(
    module: &str,
    query: &str,
    matched_symbol_names: Option<&str>,
) -> Vec<String> {
    let Some((start, end)) = named_import_bounds(module) else {
        return Vec::new();
    };
    let requested_terms = query_terms(query);
    let matched_terms = matched_symbol_names.map(query_terms).unwrap_or_default();
    let mut terms = Vec::new();
    for binding in module[start + 1..end].split(',') {
        let Some(binding_names) = import_binding_names(binding) else {
            continue;
        };
        if !import_binding_matches_terms(binding_names, &requested_terms)
            && !import_binding_matches_terms(binding_names, &matched_terms)
        {
            continue;
        }
        for term in import_usage_identifier_terms(binding_names.local) {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }

    terms
}

pub(super) fn terminal_import_binding_terms(module: &str) -> Vec<String> {
    if named_import_bounds(module).is_some() || dynamic_import_surface(module) {
        return Vec::new();
    }
    let Some(binding) = terminal_import_binding(module) else {
        return Vec::new();
    };
    let mut terms = import_usage_identifier_terms(binding);
    if import_surface_has_wildcard(module) {
        if let Some(singular) = conservative_singular_binding(binding) {
            push_import_usage_term(&mut terms, singular);
        }
    }

    terms
}

pub(super) fn import_surface_declares_local_binding(module: &str) -> bool {
    let module = module.trim_start();
    if dynamic_import_surface(module) {
        return false;
    }
    named_import_bounds(module).is_some()
        || module.starts_with("import ")
        || module.starts_with("from ")
        || module.starts_with("use ")
        || matches!(module.split_whitespace().collect::<Vec<_>>().as_slice(), [alias, path]
            if import_alias_binding(alias)
                && path.chars().any(|character| matches!(character, '/' | '.' | '\\')))
}

fn dynamic_import_surface(module: &str) -> bool {
    let module = module.trim_start();
    module.starts_with("import(")
        || module
            .strip_prefix("await ")
            .is_some_and(|module| module.trim_start().starts_with("import("))
}

fn terminal_import_binding(module: &str) -> Option<&str> {
    let module = module.trim().trim_end_matches(';').trim();
    let binding_surface = if let Some(imports) = module.strip_prefix("from ") {
        imports.split_once(" import ")?.1
    } else if let Some(imports) = module.strip_prefix("import ") {
        imports
            .split_once(" from ")
            .map_or(imports, |(binding, _)| binding)
    } else if let Some(imports) = module.strip_prefix("use ") {
        imports
    } else {
        normalized_module_binding(module)?
    };
    if binding_surface.contains(',') {
        return None;
    }
    let binding_surface = binding_surface
        .rsplit_once(" as ")
        .map_or(binding_surface, |(_, alias)| alias)
        .trim();
    let binding_surface = binding_surface
        .strip_suffix("._")
        .unwrap_or(binding_surface)
        .trim_end_matches('*')
        .trim_end_matches(['.', ':', '\\'])
        .trim();
    identifier_tokens(binding_surface).last()
}

fn normalized_module_binding(module: &str) -> Option<&str> {
    let parts = module.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [alias, path]
            if import_alias_binding(alias)
                && path
                    .chars()
                    .any(|character| matches!(character, '/' | '.' | '\\')) =>
        {
            Some(alias)
        }
        [path]
            if !path.starts_with('#')
                && !path.starts_with("./")
                && !path.starts_with("../")
                && !path.starts_with('/') =>
        {
            Some(path)
        }
        _ => None,
    }
}

fn import_alias_binding(alias: &str) -> bool {
    !matches!(alias, "." | "_")
        && alias
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && alias
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn import_surface_has_wildcard(module: &str) -> bool {
    let module = module.trim_end_matches([';', ' ']);
    module.ends_with('*') || module.ends_with("._")
}

fn conservative_singular_binding(binding: &str) -> Option<&str> {
    let singular = binding.strip_suffix('s')?;
    (singular.len() >= 4
        && !binding.ends_with("ss")
        && !binding.ends_with("us")
        && !binding.ends_with("is"))
    .then_some(singular)
}

fn named_import_bounds(module: &str) -> Option<(usize, usize)> {
    let start = module.find('{')?;
    let end = module[start + 1..].find('}')? + start + 1;
    (end > start).then_some((start, end))
}

#[derive(Clone, Copy)]
struct ImportBindingNames<'a> {
    imported: &'a str,
    local: &'a str,
}

fn import_binding_name(binding: &str) -> Option<&str> {
    import_binding_names(binding).map(|names| names.local)
}

fn import_binding_names(binding: &str) -> Option<ImportBindingNames<'_>> {
    let binding = binding.trim().trim_start_matches("type ").trim();
    let imported = binding
        .split_whitespace()
        .next()?
        .trim_matches(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
    let binding_name = binding
        .split_whitespace()
        .last()?
        .trim_matches(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
    (!imported.is_empty() && !binding_name.is_empty()).then_some(ImportBindingNames {
        imported,
        local: binding_name,
    })
}

fn import_binding_matches_query(binding_name: &str, query_terms: &[String]) -> bool {
    query_terms
        .iter()
        .any(|term| identifier_terms_equivalent(binding_name, term))
}

fn import_binding_matches_terms(binding_names: ImportBindingNames<'_>, terms: &[String]) -> bool {
    import_binding_matches_query(binding_names.imported, terms)
        || import_binding_matches_query(binding_names.local, terms)
}

pub(super) fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn query_local_binding_terms(query: &str) -> Vec<String> {
    let terms = query_terms(query);
    let last_index = terms.len().checked_sub(1);
    let mut bindings = terms
        .into_iter()
        .enumerate()
        .filter(|(index, term)| {
            Some(*index) == last_index || term.contains('_') || term_has_case_boundary(term)
        })
        .map(|(_, term)| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    bindings.sort();
    bindings.dedup();
    bindings
}

pub(super) fn import_usage_identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in identifier_tokens(value) {
        if import_usage_term_is_specific(token) {
            push_import_usage_term(&mut terms, token);
        }
        for part in token.split('_').filter(|part| !part.is_empty()) {
            if import_usage_term_is_specific(part) {
                push_import_usage_term(&mut terms, part);
            }
        }
        for term in camel_case_terms(token) {
            if import_usage_term_is_specific(&term) {
                push_import_usage_term(&mut terms, &term);
            }
        }
    }

    terms
}

fn push_import_usage_term(terms: &mut Vec<String>, term: &str) {
    let term = term.to_ascii_lowercase();
    if !terms.contains(&term) {
        terms.push(term);
    }
}

fn import_usage_term_is_specific(term: &str) -> bool {
    term.len() >= 5 || term.contains('_') || term_has_case_boundary(term)
}

fn term_has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

pub(super) fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

pub(super) fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

#[cfg(test)]
#[path = "binding_terms_tests.rs"]
mod tests;
