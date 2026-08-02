use super::super::lexical::python_strings::extract_quoted_string_python;

pub(super) const DYNAMIC_PYTHON_MOUNT_PREFIX: &str = "\0dynamic";

#[cfg(test)]
#[path = "arguments_tests.rs"]
mod tests;

fn split_python_top_level_arguments(args: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in args.char_indices() {
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
            '\'' | '"' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = args[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let argument = args[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

pub(super) fn parse_flask_methods_decorator(line: &str) -> Option<Vec<String>> {
    let line = line.trim_start_matches('@');
    let paren_pos = line.find('(')?;
    let (func_part, args) = (&line[..paren_pos], &line[paren_pos + 1..]);
    if func_part != ".methods" {
        let base = func_part.rsplit('.').next().unwrap_or("");
        if base != "methods" {
            return None;
        }
    }
    let args_trimmed = trim_one_trailing_paren(args);
    Some(extract_explicit_methods_list_python(args_trimmed))
}

pub(super) fn trim_one_trailing_paren(args: &str) -> &str {
    let trimmed = args.trim_end();
    trimmed.strip_suffix(')').unwrap_or(trimmed)
}

pub(super) fn extract_methods_from_flask_args(args: &str) -> Vec<String> {
    let Some(list_str) = extract_python_keyword_value(args, "methods") else {
        return extract_shorthand_method_from_route(args);
    };
    extract_explicit_methods_list_python(list_str)
}

fn extract_explicit_methods_list_python(args: &str) -> Vec<String> {
    let methods = extract_methods_list_python(args);
    if methods.is_empty() {
        vec!["any".to_owned()]
    } else {
        methods
    }
}

fn extract_python_keyword_string(args: &str, keyword: &str) -> Option<String> {
    extract_python_keyword_value(args, keyword).and_then(extract_quoted_string_python)
}

pub(super) fn python_prefix_argument(args: &str, keyword: &str) -> String {
    if let Some(prefix) = extract_python_keyword_string(args, keyword) {
        return prefix;
    }
    if extract_python_keyword_value(args, keyword).is_some() {
        return DYNAMIC_PYTHON_MOUNT_PREFIX.to_owned();
    }
    String::new()
}

pub(super) fn extract_python_router_argument(args: &str, keyword: &str) -> Option<String> {
    extract_python_keyword_value(args, keyword)
        .and_then(python_handler_name_from_value)
        .or_else(|| {
            split_python_top_level_arguments(args)
                .into_iter()
                .find(|argument| !argument.contains('='))
                .and_then(python_handler_name_from_value)
        })
}

pub(super) fn extract_python_keyword_value<'a>(args: &'a str, keyword: &str) -> Option<&'a str> {
    for argument in split_python_top_level_arguments(args) {
        let argument = argument.trim_start();
        let Some(after_keyword) = argument.strip_prefix(keyword) else {
            continue;
        };
        if let Some(after_eq) = after_keyword.strip_prefix('=') {
            return Some(after_eq.trim_start());
        }
        if after_keyword
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            let after_name = after_keyword.trim_start();
            if let Some(after_eq) = after_name.strip_prefix('=') {
                return Some(after_eq.trim_start());
            }
        }
    }
    None
}

pub(super) fn extract_python_add_url_rule_positional_handler(args: &str) -> Option<String> {
    let arguments = split_python_top_level_arguments(args);
    let value = arguments.get(2)?.trim();
    if value.contains('=') {
        return None;
    }
    python_handler_name_from_value(value)
}

pub(super) fn python_handler_name_from_value(value: &str) -> Option<String> {
    let value = value.trim_start();
    if value.starts_with("lambda") || value.starts_with('(') {
        return None;
    }
    let name_end = value
        .find(|character: char| {
            character == '(' || character == ')' || character == ',' || character.is_whitespace()
        })
        .unwrap_or(value.len());
    let dotted_name = &value[..name_end];
    let mut parts = dotted_name
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.last() == Some(&"as_view") && parts.len() > 1 {
        parts.pop();
    }
    let name = parts.last().copied().unwrap_or("");
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(name.to_owned())
}

pub(super) fn extract_python_route_path(args: &str) -> Option<String> {
    extract_python_keyword_string(args, "path")
        .or_else(|| extract_python_keyword_string(args, "rule"))
        .or_else(|| extract_quoted_string_python(args))
}

fn extract_shorthand_method_from_route(args: &str) -> Vec<String> {
    let first_part = args.split(',').next().unwrap_or("");
    let after_close = first_part.find(')');
    let relevant = match after_close {
        Some(pos) => &first_part[..pos],
        None => first_part,
    };
    let url = extract_quoted_string_python(relevant);
    let Some(url) = url else {
        return Vec::new();
    };
    let after_url_byte_count = relevant.find(&url).map(|start| start + url.len() + 1);
    let Some(after_url_pos) = after_url_byte_count else {
        return vec!["get".to_owned()];
    };
    let remaining = relevant.get(after_url_pos..).unwrap_or("").trim();
    if remaining.starts_with(')') || remaining.starts_with(',') || remaining.is_empty() {
        return vec!["get".to_owned()];
    }
    if remaining.starts_with('"') || remaining.starts_with('\'') {
        if let Some(method) = extract_quoted_string_python(remaining) {
            let method = method.to_ascii_lowercase();
            if matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            ) {
                return vec![method];
            }
        }
    }
    vec!["get".to_owned()]
}

fn extract_methods_list_python(args: &str) -> Vec<String> {
    let trimmed = args.trim();
    let inner = python_collection_literal_inner(trimmed).unwrap_or(trimmed);
    let mut methods = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if let Some(method) = extract_quoted_string_python(item) {
            let method = method.to_ascii_lowercase();
            if matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            ) {
                methods.push(method);
            }
        }
    }
    methods
}

fn python_collection_literal_inner(value: &str) -> Option<&str> {
    let trimmed = value.trim_start();
    let (open_pos, close_char) =
        trimmed
            .char_indices()
            .find_map(|(index, character)| match character {
                '[' => Some((index, ']')),
                '(' => Some((index, ')')),
                '{' => Some((index, '}')),
                _ => None,
            })?;
    let close_pos = trimmed.rfind(close_char)?;
    if close_pos <= open_pos {
        return None;
    }
    Some(&trimmed[open_pos + 1..close_pos])
}
