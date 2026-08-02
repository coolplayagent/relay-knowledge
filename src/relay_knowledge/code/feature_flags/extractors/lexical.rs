pub(super) fn find_pattern_with_quotes(
    line: &str,
    pattern: &str,
    start: usize,
    quote_predicate: fn(char) -> bool,
) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        let character = rest.chars().next()?;
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            index = index.saturating_add(character.len_utf8());
            continue;
        }

        if index >= start && rest.starts_with(pattern) {
            return Some(index);
        }
        if quote_predicate(character) {
            quote = Some(character);
        }
        index = index.saturating_add(character.len_utf8());
    }

    None
}
pub(super) fn is_quote_character(character: char) -> bool {
    matches!(character, '"' | '\'' | '`')
}
pub(super) fn valid_source_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 160
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}
pub(super) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[cfg(test)]
#[path = "lexical_tests.rs"]
mod tests;
