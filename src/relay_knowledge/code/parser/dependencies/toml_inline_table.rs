//! Reads fields from dependency declarations encoded as TOML inline tables.

pub(super) fn inline_table_field(value: &str, field: &str) -> Option<String> {
    let after_equals = inline_table_value(value, field)?;
    let quote = after_equals.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = after_equals.get(1..)?.find(quote)?;
    Some(after_equals[1..1 + end].to_owned())
}

pub(super) fn inline_table_bool_field(value: &str, field: &str) -> Option<bool> {
    let after_equals = inline_table_value(value, field)?;
    if after_equals.starts_with("true") {
        Some(true)
    } else if after_equals.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn inline_table_value<'a>(value: &'a str, field: &str) -> Option<&'a str> {
    let mut start = 0;
    let body = inline_table_body(value);
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' && active_quote == '"' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == ',' {
            if let Some(result) = inline_table_entry_value(&body[start..index], field) {
                return Some(result);
            }
            start = index + character.len_utf8();
        }
    }
    inline_table_entry_value(&body[start..], field)
}

fn inline_table_body(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(value)
        .trim()
}

fn inline_table_entry_value<'a>(entry: &'a str, field: &str) -> Option<&'a str> {
    let (key, raw_value) = entry.split_once('=')?;
    if key.trim() == field {
        Some(raw_value.trim())
    } else {
        None
    }
}
