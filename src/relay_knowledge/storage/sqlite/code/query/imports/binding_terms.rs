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
