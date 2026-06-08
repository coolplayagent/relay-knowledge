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
    route_arguments_after_path(rest)
        .into_iter()
        .filter_map(handler_name_from_argument)
        .next_back()
}

fn route_arguments_after_path(rest: &str) -> Vec<&str> {
    let Some(rest) = rest.strip_prefix(',') else {
        return Vec::new();
    };
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
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
    arguments
}

fn handler_name_from_argument(argument: &str) -> Option<String> {
    let argument = argument.trim_start();
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
    Some(argument[..end].to_owned())
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
