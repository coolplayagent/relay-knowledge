pub(in crate::code::parser) fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote_char = s.chars().next()?;
    if quote_char != '\'' && quote_char != '"' && quote_char != '`' {
        return None;
    }
    let inner = &s[1..];
    let end = inner.find(quote_char)?;
    Some(inner[..end].to_owned())
}

pub(in crate::code::parser) fn extract_handler_name(s: &str) -> Option<String> {
    let quote_char = s.chars().next()?;
    let after_first_string = &s[1..];
    let close_pos = after_first_string.find(quote_char)?;
    let rest = after_first_string[close_pos + 1..].trim_start();
    let rest = rest.strip_prefix(',')?.trim_start();
    let is_func = rest
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    if !is_func {
        return None;
    }
    let end = rest
        .find(|c: char| c == ')' || c == ',' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

pub(in crate::code::parser) fn extract_quoted_string_python(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote_char = s.chars().next()?;
    if quote_char != '\'' && quote_char != '"' {
        return None;
    }
    let inner = &s[1..];
    let mut result = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            result.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == quote_char {
            return Some(result);
        }
        result.push(c);
    }
    Some(result)
}
