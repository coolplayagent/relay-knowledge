use tree_sitter::Node;

use super::super::super::recovery::{
    c_family_typedef_like_function_signature, decorated_function_error_body_is_statement_like,
    decorated_function_head_has_recoverable_tail, decorated_function_head_has_recovery_decorator,
    decorated_function_head_text, scan::scan_code_line_indices,
};
use super::{
    SyntaxRange, c_declaration_prefix_token, c_identifier_char, data_symbol_name, node_text,
    syntax_range,
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
    if !decorated_function_head_has_recovery_decorator(head)
        || !decorated_function_head_has_recoverable_tail(head, false, false, false)
        || !decorated_function_error_body_is_statement_like(&text)
    {
        return None;
    }
    let name = c_function_name_from_decorated_head(head)?;
    (c_family_typedef_like_function_signature(head)
        && c_decorated_function_head_is_declaration(head, &name))
    .then(|| (name, "function", syntax_range(node)))
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
    if name.is_empty() {
        return false;
    }
    let Some((name_start, name_end)) = c_function_name_bounds_from_decorated_head(head) else {
        return false;
    };
    &head[name_start..name_end] == name
}
