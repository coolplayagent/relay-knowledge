use super::identity::has_case_boundary;

pub(super) fn call_display_name(
    name: Option<&str>,
    canonical_symbol_id: Option<&str>,
) -> Option<String> {
    let canonical_symbol_id = canonical_symbol_id
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let terms = canonical_symbol_id
        .map(display_identity_terms)
        .unwrap_or_default();
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| terms.last().copied())?;
    if terms.is_empty() {
        return Some(name.to_owned());
    }
    let name_index = terms.iter().rposition(|term| *term == name)?;
    let owner = terms.get(name_index.checked_sub(1)?)?;
    if *owner == name || !display_owner_term(owner) || !generic_nested_display_name(name) {
        return Some(name.to_owned());
    }

    Some(format!("{owner}.{name}"))
}

fn generic_nested_display_name(name: &str) -> bool {
    matches!(
        name,
        "callback"
            | "client"
            | "connection"
            | "event"
            | "handler"
            | "item"
            | "request"
            | "response"
            | "source"
            | "stream"
            | "value"
    )
}

pub(super) fn inferred_caller_name_from_excerpt(caller_excerpt: Option<&str>) -> Option<String> {
    caller_excerpt?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(caller_name_from_declaration_line)
}

fn caller_name_from_declaration_line(line: &str) -> Option<String> {
    let open = line.find('(')?;
    let before_open = line[..open].trim_end();
    let end = before_open
        .char_indices()
        .rev()
        .find(|(_, character)| identifier_character(*character))
        .map(|(index, character)| index + character.len_utf8())?;
    let start = before_open[..end]
        .char_indices()
        .rev()
        .find(|(_, character)| !identifier_character(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let name = before_open[start..end].trim();
    if name.is_empty() || declaration_caller_name_is_control_keyword(name) {
        None
    } else {
        Some(name.to_owned())
    }
}

fn declaration_caller_name_is_control_keyword(name: &str) -> bool {
    matches!(name, "catch" | "for" | "if" | "switch" | "while" | "with")
}

pub(super) fn identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn display_owner_term(term: &str) -> bool {
    term.chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        || has_case_boundary(term)
        || term.contains('_')
}

fn display_identity_terms(value: &str) -> Vec<&str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
