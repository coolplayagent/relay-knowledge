//! C++ class and member-function declarator recognition.

use super::{
    source_text::{line_code_without_comment, next_character},
    top_level_scan::{
        identifier_continue, identifier_spans, identifier_spans_outside_groups, identifier_start,
        top_level_body_open_start, top_level_character_start,
    },
};

pub(super) fn cpp_class_header_opens_body(line: &str) -> bool {
    top_level_body_open_start(line).is_some_and(|body_start| {
        let header = &line[..body_start];
        cpp_class_header_starts(header)
    })
}

pub(super) fn cpp_class_header_starts(code: &str) -> bool {
    cpp_class_header_name(code).is_some()
}

pub(super) fn cpp_class_header_name(header: &str) -> Option<String> {
    let search_end = top_level_body_open_start(header).unwrap_or(header.len());
    let header = &header[..search_end];
    let identifiers = identifier_spans(header);
    let mut name = None;
    for (position, (start, end)) in identifiers.iter().copied().enumerate() {
        let token = &header[start..end];
        if !matches!(token, "class" | "struct") {
            continue;
        }
        if position > 0 {
            let (previous_start, previous_end) = identifiers[position - 1];
            if &header[previous_start..previous_end] == "enum" {
                continue;
            }
        }
        if let Some(candidate) = class_declarator_name(&header[end..]) {
            name = Some(candidate);
        }
    }
    name
}

fn class_declarator_name(declarator: &str) -> Option<String> {
    let declarator_end = top_level_class_declarator_end(declarator);
    identifier_spans_outside_groups(&declarator[..declarator_end])
        .into_iter()
        .filter_map(|(start, end)| {
            let candidate = &declarator[start..end];
            (function_name_candidate(candidate) && !class_declarator_noise_token(candidate))
                .then(|| candidate.to_owned())
        })
        .next_back()
}

fn top_level_class_declarator_end(declarator: &str) -> usize {
    top_level_character_start(declarator, ':').unwrap_or(declarator.len())
}

fn class_declarator_noise_token(token: &str) -> bool {
    matches!(
        token,
        "class"
            | "struct"
            | "typename"
            | "final"
            | "sealed"
            | "abstract"
            | "alignas"
            | "__attribute__"
            | "attribute"
            | "__declspec"
            | "__declspec__"
    ) || (token.contains('_') && uppercase_decorator_name(token))
}

pub(super) fn member_function_declaration_name(statement: &str) -> Option<String> {
    let code = statement
        .lines()
        .map(line_code_without_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if special_member_assignment(&code, "delete")
        || special_member_assignment(&code, "default")
        || code.starts_with("using ")
    {
        return None;
    }
    let parameter_start = top_level_parameter_start(&code)?;
    let (name_start, name_end) = name_bounds_before_open(&code, parameter_start)?;
    if code[..name_start].trim_end().ends_with('~')
        || contains_operator_keyword(&code[..parameter_start])
    {
        return None;
    }
    let name = &code[name_start..name_end];
    function_name_candidate(name).then(|| name.to_owned())
}

fn special_member_assignment(code: &str, keyword: &str) -> bool {
    let code = code.trim_end().trim_end_matches(';').trim_end();
    let Some(prefix) = code.strip_suffix(keyword) else {
        return false;
    };
    prefix.trim_end().ends_with('=')
}

fn contains_operator_keyword(prefix: &str) -> bool {
    prefix
        .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .any(|token| token == "operator")
}

fn top_level_parameter_start(code: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        match character {
            '(' if depth == 0 && parameter_open_looks_like_function(code, index) => {
                if let Some(after_decorator) = parameter_decorator_end(code, index) {
                    index = after_decorator;
                    continue;
                }
                return Some(index);
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += character.len_utf8();
    }

    None
}

fn parameter_decorator_end(code: &str, parameter_start: usize) -> Option<usize> {
    let (name_start, name_end) = name_bounds_before_open(code, parameter_start)?;
    let name = &code[name_start..name_end];
    if !member_decorator_name(name) {
        return None;
    }
    let group_end = matching_parameter_end(code, parameter_start)?;
    let rest = code[group_end + ")".len()..].trim_start();
    (rest.contains('(') && rest.trim_end_matches(';').chars().any(identifier_start))
        .then_some(group_end + ")".len())
}

fn member_decorator_name(name: &str) -> bool {
    matches!(
        name,
        "__attribute__"
            | "attribute"
            | "__declspec"
            | "__declspec__"
            | "__always_inline"
            | "always_inline"
    ) || uppercase_decorator_name(name)
}

fn uppercase_decorator_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

fn matching_parameter_end(code: &str, parameter_start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = parameter_start;
    let mut string_delimiter = None;
    let mut escaped = false;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        if let Some(delimiter) = string_delimiter {
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => string_delimiter = Some(character),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += character.len_utf8();
    }

    None
}

fn parameter_open_looks_like_function(code: &str, parameter_start: usize) -> bool {
    if code[parameter_start + 1..]
        .trim_start()
        .starts_with(['*', '&'])
    {
        return false;
    }
    if code[..parameter_start].trim_end().len() != parameter_start {
        return false;
    }
    name_bounds_before_open(code, parameter_start)
        .map(|(start, end)| function_name_candidate(&code[start..end]))
        .unwrap_or(false)
}

fn name_bounds_before_open(code: &str, parameter_start: usize) -> Option<(usize, usize)> {
    let name_end = code[..parameter_start].trim_end().len();
    let name_start = code[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some((name_start, name_end))
}

fn function_name_candidate(name: &str) -> bool {
    if matches!(
        name,
        "if" | "for" | "while" | "switch" | "return" | "sizeof" | "void"
    ) {
        return false;
    }
    let mut characters = name.chars();
    characters.next().is_some_and(identifier_start) && characters.all(identifier_continue)
}

#[cfg(test)]
#[path = "declarators_tests.rs"]
mod tests;
