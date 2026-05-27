use tree_sitter::Node;

use super::super::recovery::{decorated_function_head_text, scan::scan_code_line_indices};
use super::{
    SyntaxRange, c_declaration_prefix_token, c_declaration_type_token, c_identifier_char,
    data_symbol_name, node_text, syntax_range,
};

pub(super) fn gcc_decorated_function_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let text = node_text(content, node);
    if !text.contains('{') {
        return None;
    }
    let head = decorated_function_head_text(&text)?;
    if !c_function_head_has_recovery_decorator(head) {
        return None;
    }
    let name = c_function_name_from_decorated_head(head)?;
    c_decorated_function_head_is_declaration(head, &name)
        .then(|| (name, "function", syntax_range(node)))
}

fn c_function_head_has_recovery_decorator(head: &str) -> bool {
    let Some(parameter_start) = c_top_level_parameter_start(head) else {
        return false;
    };
    let parameter_end = c_closing_parenthesis_index(&head[parameter_start..])
        .map_or(head.len(), |index| parameter_start + index + 1);
    head[..parameter_start]
        .split(|character: char| !c_identifier_char(character))
        .chain(head[parameter_end..].split(|character: char| !c_identifier_char(character)))
        .any(c_recovery_decorator_token)
}

fn c_recovery_decorator_token(token: &str) -> bool {
    matches!(
        token,
        "__always_inline"
            | "__attribute__"
            | "__attribute"
            | "__declspec"
            | "__declspec__"
            | "__inline"
            | "__inline__"
            | "always_inline"
            | "attribute"
    )
}

fn c_function_name_from_decorated_head(head: &str) -> Option<String> {
    let (name_start, name_end) = c_function_name_bounds_from_decorated_head(head)?;
    let name = &head[name_start..name_end];

    data_symbol_name(name).then(|| name.to_owned())
}

fn c_function_name_bounds_from_decorated_head(head: &str) -> Option<(usize, usize)> {
    let parameter_start = c_top_level_parameter_start(head)?;
    let name_end = head[..parameter_start].trim_end().len();
    let name_start = head[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some((name_start, name_end))
}

fn c_top_level_parameter_start(head: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut candidate = None;
    let literals_closed = scan_code_line_indices(head, |index, character| match character {
        '(' => {
            if depth == 0 && c_parameter_open_looks_like_declarator(head, index) {
                candidate = Some(index);
            }
            depth += 1;
        }
        ')' => depth = depth.saturating_sub(1),
        _ => {}
    });

    literals_closed.then_some(candidate).flatten()
}

fn c_closing_parenthesis_index(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0isize;
    let mut matched_end = None;
    let literals_closed = scan_code_line_indices(text, |index, character| {
        if matched_end.is_some() {
            return;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    matched_end = Some(index);
                }
            }
            _ => {}
        }
    });

    literals_closed.then_some(matched_end).flatten()
}

fn c_parameter_open_looks_like_declarator(head: &str, parameter_start: usize) -> bool {
    let Some((name_start, name_end)) = c_name_bounds_before_open(head, parameter_start) else {
        return false;
    };
    let token = &head[name_start..name_end];
    data_symbol_name(token) && !c_declaration_prefix_token(token)
}

fn c_name_bounds_before_open(head: &str, parameter_start: usize) -> Option<(usize, usize)> {
    let name_end = head[..parameter_start].trim_end().len();
    let name_start = head[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some((name_start, name_end))
}

fn c_decorated_function_head_is_declaration(head: &str, name: &str) -> bool {
    if head.contains('=') || name.is_empty() {
        return false;
    }
    let Some((name_start, name_end)) = c_function_name_bounds_from_decorated_head(head) else {
        return false;
    };
    if &head[name_start..name_end] != name {
        return false;
    }
    let tokens = head[..name_start]
        .split(|character: char| !c_identifier_char(character))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some(type_index) = tokens
        .iter()
        .rposition(|token| !c_declaration_prefix_token(token))
    else {
        return false;
    };
    if !c_declaration_type_token(tokens[type_index]) {
        return false;
    }

    tokens[..type_index]
        .iter()
        .all(|token| c_declaration_prefix_token(token) || c_declaration_type_token(token))
}
