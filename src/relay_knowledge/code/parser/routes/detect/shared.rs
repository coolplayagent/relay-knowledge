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
    split_top_level_arguments(s)
        .into_iter()
        .skip(1)
        .filter_map(handler_name_from_argument)
        .next_back()
}

pub(in crate::code::parser) fn extract_handler_name_from_arguments(s: &str) -> Option<String> {
    split_top_level_arguments(s)
        .into_iter()
        .filter_map(handler_name_from_argument)
        .next_back()
}

fn split_top_level_arguments(rest: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut closed = false;
    for (index, character) in rest.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let argument = rest[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                closed = true;
                break;
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = rest[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if !closed {
        let argument = rest[argument_start..].trim();
        if !argument.is_empty() {
            arguments.push(argument);
        }
    }
    arguments
}

fn handler_name_from_argument(argument: &str) -> Option<String> {
    let argument = argument.trim_start();
    if let Some(inner) = javascript_array_argument_inner(argument) {
        return split_top_level_arguments(inner)
            .into_iter()
            .filter_map(handler_name_from_argument)
            .next_back();
    }
    if inline_callback_argument(argument) {
        return None;
    }
    let is_func = argument
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    if !is_func {
        return None;
    }
    let end = argument
        .find(|c: char| c == ')' || c == ',' || c.is_whitespace())
        .unwrap_or(argument.len());
    let name = argument[..end].rsplit('.').next().unwrap_or("").trim();
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '$'
        })
    {
        return None;
    }
    Some(name.to_owned())
}

fn javascript_array_argument_inner(argument: &str) -> Option<&str> {
    let argument = argument.trim();
    if !argument.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in argument.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => quote = Some(character),
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&argument[1..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn inline_callback_argument(argument: &str) -> bool {
    argument.starts_with('(')
        || strip_javascript_keyword(argument, "function").is_some()
        || strip_javascript_keyword(argument, "async").is_some_and(|after_async| {
            let after_async = after_async.trim_start();
            after_async.starts_with('(')
                || strip_javascript_keyword(after_async, "function").is_some()
        })
}

fn strip_javascript_keyword<'a>(argument: &'a str, keyword: &str) -> Option<&'a str> {
    let after_keyword = argument.strip_prefix(keyword)?;
    if after_keyword
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(after_keyword)
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
