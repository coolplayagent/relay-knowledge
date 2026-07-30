use std::collections::BTreeSet;

use super::super::javascript::find_javascript_pattern_outside_strings;
use super::arguments::extract_quoted_string;

pub(super) fn express_method_position(line: &str) -> Option<usize> {
    [
        ".get(",
        ".post(",
        ".put(",
        ".delete(",
        ".patch(",
        ".head(",
        ".options(",
        ".all(",
    ]
    .into_iter()
    .filter_map(|method| find_javascript_pattern_outside_strings(line, method))
    .min()
}

pub(super) fn express_route_start_position(line: &str) -> Option<usize> {
    [
        express_method_position(line),
        find_javascript_pattern_outside_strings(line, ".route("),
    ]
    .into_iter()
    .flatten()
    .min()
}

pub(super) fn express_http_method(raw_method: &str) -> Option<String> {
    let method = raw_method.to_ascii_lowercase();
    match method.as_str() {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => Some(method),
        "all" => Some("any".to_owned()),
        _ => None,
    }
}

pub(super) fn express_route_urls(arguments: &str) -> Vec<String> {
    let Some(first_argument) = first_top_level_argument(arguments) else {
        return Vec::new();
    };
    if let Some(url) = extract_quoted_string(first_argument) {
        return route_url_literals([url]);
    }
    let Some(array_inner) = javascript_array_literal_inner(first_argument) else {
        return Vec::new();
    };
    route_url_literals(extract_quoted_strings(array_inner))
}

pub(super) fn route_url_literals(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    urls.into_iter()
        .filter(|url| url.starts_with('/') && !url.contains("${"))
        .collect()
}

pub(super) fn javascript_call_end(arguments: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
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
            ')' if depth == 0 => return Some(index + character.len_utf8()),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

pub(super) fn javascript_array_literal_inner(value: &str) -> Option<&str> {
    let value = value.trim();
    if !value.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
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
                    return Some(&value[1..index]);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn extract_quoted_strings(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(start_relative) = value[offset..].find(['\'', '"', '`']) {
        let start = offset + start_relative;
        if let Some(url) = extract_quoted_string(&value[start..]) {
            offset = start + url.len() + 2;
            values.push(url);
        } else {
            break;
        }
    }
    values
}

pub(super) fn express_receiver_name(receiver: &str) -> Option<String> {
    receiver
        .rsplit(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == '$')
        })
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

pub(super) fn express_router_name_is_router(
    receiver_name: &str,
    router_names: &BTreeSet<String>,
) -> bool {
    if router_names.contains(receiver_name) {
        return true;
    }
    let receiver_name = receiver_name.to_ascii_lowercase();

    receiver_name == "app" || receiver_name == "router"
}

pub(super) fn javascript_top_level_arguments(rest: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
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
                return arguments;
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
    let argument = rest[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

pub(super) fn merge_url_parts(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return if suffix.starts_with('/') {
            suffix.to_owned()
        } else {
            format!("/{suffix}")
        };
    }
    if suffix.is_empty() {
        return prefix.to_owned();
    }
    let prefix = prefix.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    format!("{prefix}/{suffix}")
}

fn first_top_level_argument(arguments: &str) -> Option<&str> {
    let arguments = arguments.trim_start();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
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
            ')' | ',' if depth == 0 => {
                let argument = arguments[..index].trim();
                return (!argument.is_empty()).then_some(argument);
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let argument = arguments.trim();
    (!argument.is_empty()).then_some(argument)
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
