pub(in crate::code::parser) fn extract_quoted_string_python(source: &str) -> Option<String> {
    let source = source.trim_start();
    let string_start = python_static_string_start(source)?;
    let source = &source[string_start..];
    let quote_char = source.chars().next()?;
    if quote_char != '\'' && quote_char != '"' {
        return None;
    }
    let inner = &source[1..];
    let mut result = String::new();
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            result.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote_char {
            return Some(result);
        }
        result.push(character);
    }
    Some(result)
}

fn python_static_string_start(source: &str) -> Option<usize> {
    let quote_index = source.find(['\'', '"'])?;
    let prefix = &source[..quote_index];
    if prefix
        .chars()
        .all(|character| matches!(character.to_ascii_lowercase(), 'r' | 'u' | 'b'))
    {
        Some(quote_index)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "python_strings_tests.rs"]
mod tests;
