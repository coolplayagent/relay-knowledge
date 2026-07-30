use super::nodes::{self, push_children_reverse};
use tree_sitter::Node;
mod declaration;
mod line_classification;
pub(in crate::code::parser) mod scan;
mod signature;
mod type_body;
use declaration::{
    c_family_typedef_like_initializer_declaration, c_identifier_char, c_identifier_name,
};
use line_classification::{
    c_family_decorated_type_line, c_family_macro_name,
    c_family_recoverable_error_function_signature,
};
use scan::{CodeScanState, first_code_char_index, scan_code_line_indices_with_state};
pub(super) use scan::{
    code_contains_char, scan_code_line_indices, token_starts_in_angle_arguments,
};
use signature::c_family_typedef_like_function_signature_with_options;
pub(in crate::code::parser) use signature::{
    c_family_typedef_like_function_signature, decorated_function_head_has_recoverable_tail,
    decorated_function_head_has_recovery_decorator,
};
use type_body::decorated_type_error_body_is_declaration_like;
const MAX_RECOVERABLE_DECORATED_TYPE_ERROR_LINES: usize = 120;

pub(super) fn recoverable_c_family_parse(
    language_id: &str,
    root: Node<'_>,
    content: &str,
    has_structured_facts: bool,
) -> bool {
    if !matches!(language_id, "c" | "cpp") || !has_structured_facts {
        return false;
    }
    let mut saw_error = false;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if syntax_error_node(node) {
            saw_error = true;
            if !recoverable_c_family_error(language_id, content, node) {
                return false;
            }
        }
        push_children_reverse(node, &mut stack);
    }

    saw_error
}

fn syntax_error_node(node: Node<'_>) -> bool {
    node.is_error() || node.is_missing() || node.kind() == "ERROR"
}

fn recoverable_c_family_error(language_id: &str, content: &str, node: Node<'_>) -> bool {
    let range = nodes::syntax_range(node);
    if recoverable_missing_declarator_after_decorated_type(content, node) {
        return true;
    }
    if recoverable_decorated_function_error(language_id, content, node) {
        return true;
    }
    let mut ancestor = node;
    while let Some(parent) = ancestor.parent() {
        if recoverable_decorated_function_error(language_id, content, parent) {
            return true;
        }
        if language_id == "cpp"
            && parent.kind() == "qualified_identifier"
            && source_line(content, nodes::syntax_range(parent).line_start)
                .is_some_and(c_family_typedef_like_error_line)
        {
            return true;
        }
        ancestor = parent;
    }
    if range.line_end.saturating_sub(range.line_start) > 2 {
        return recoverable_decorated_type_error(content, node, &range);
    }
    if recoverable_preprocessor_error(content, node, &range) {
        return true;
    }
    source_line(content, range.line_start).is_some_and(recoverable_c_family_error_line)
}

fn recoverable_decorated_function_error(language_id: &str, content: &str, node: Node<'_>) -> bool {
    content
        .get(node.start_byte()..node.end_byte())
        .is_some_and(|text| {
            recoverable_decorated_function_error_text_with_options(
                text,
                language_id == "cpp",
                language_id == "cpp",
                language_id == "cpp",
            )
        })
}

#[cfg(test)]
pub(super) fn recoverable_decorated_function_error_text(text: &str) -> bool {
    recoverable_decorated_function_error_text_with_options(text, true, true, true)
}

fn recoverable_decorated_function_error_text_with_options(
    text: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.contains('{') || !trimmed.ends_with('}') {
        return false;
    }
    let Some(head) = decorated_function_head_text(trimmed) else {
        return false;
    };
    if !decorated_function_head_has_recovery_decorator(head) {
        return false;
    }
    if !c_family_typedef_like_function_signature_with_options(
        head,
        allow_default_arguments,
        allow_cpp_method_suffix,
        allow_operator_declarator,
    ) {
        return false;
    }
    decorated_function_error_body_is_statement_like(trimmed)
}

pub(super) fn decorated_function_head_text(text: &str) -> Option<&str> {
    let open_brace = first_code_char_index(text, '{')?;
    Some(text[..open_brace].trim())
}

pub(in crate::code::parser) fn decorated_function_error_body_is_statement_like(text: &str) -> bool {
    let Some(open_brace) = first_code_char_index(text, '{') else {
        return false;
    };
    let Some(close_brace) = text.rfind('}') else {
        return false;
    };
    if close_brace <= open_brace {
        return false;
    }

    let mut brace_depth = 0isize;
    let mut parentheses = 0isize;
    let mut brackets = 0isize;
    let mut scan_state = CodeScanState::default();
    let mut pending_assignment = false;
    for line in text[open_brace + 1..close_brace].lines() {
        let continued_before = parentheses > 0 || brackets > 0;
        let mut code = String::new();
        let mut invalid_order = false;
        let mut empty_assignment = false;
        scan_code_line_indices_with_state(line, &mut scan_state, |_, character| {
            code.push(character);
            match character {
                '(' => parentheses += 1,
                ')' => parentheses -= 1,
                '[' => brackets += 1,
                ']' => brackets -= 1,
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
            if pending_assignment && !character.is_ascii_whitespace() {
                empty_assignment |= character == ';';
                pending_assignment = false;
            }
            if character == '=' {
                pending_assignment = true;
            }
            if parentheses < 0 || brackets < 0 || brace_depth < 0 {
                invalid_order = true;
            }
        });
        if invalid_order || !scan_state.line_complete() {
            return false;
        }
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if empty_assignment {
            return false;
        }
        let continued_after = parentheses > 0 || brackets > 0;
        if !continued_before
            && !continued_after
            && !decorated_function_error_body_line_is_statement_like(trimmed)
        {
            return false;
        }
    }

    scan_state.closed()
        && !pending_assignment
        && brace_depth == 0
        && parentheses == 0
        && brackets == 0
}

fn decorated_function_error_body_line_is_statement_like(line: &str) -> bool {
    !c_family_invalid_code_token_line(line)
        && (line.starts_with('#')
            || line.ends_with(';')
            || line.ends_with('{')
            || line.ends_with('}')
            || line.starts_with('}')
            || c_family_statement_label_line(line))
}

pub(super) fn c_family_invalid_code_token_line(line: &str) -> bool {
    line.chars().any(|character| matches!(character, '@' | '`'))
}

fn c_family_statement_label_line(line: &str) -> bool {
    let Some(label) = line.strip_suffix(':').map(str::trim_end) else {
        return false;
    };
    label == "default" || label.starts_with("case ") || c_identifier_name(label)
}

fn recoverable_decorated_type_error(
    content: &str,
    node: Node<'_>,
    range: &nodes::SyntaxRange,
) -> bool {
    if range.line_end.saturating_sub(range.line_start) > MAX_RECOVERABLE_DECORATED_TYPE_ERROR_LINES
    {
        return false;
    }
    if !source_line(content, range.line_start).is_some_and(c_family_decorated_type_line) {
        return false;
    }

    content
        .get(node.start_byte()..node.end_byte())
        .is_some_and(recoverable_decorated_type_error_text)
}

pub(super) fn recoverable_decorated_type_error_text(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.contains('{') || !(trimmed.ends_with("};") || trimmed.ends_with('}')) {
        return false;
    }

    decorated_type_error_body_is_declaration_like(trimmed)
}

fn recoverable_missing_declarator_after_decorated_type(content: &str, node: Node<'_>) -> bool {
    if !node.is_missing() || node.kind() != "identifier" {
        return false;
    }
    let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == "declaration")
    else {
        return false;
    };
    content
        .get(parent.start_byte()..parent.end_byte())
        .is_some_and(|text| {
            text.lines()
                .find(|line| !line.trim().is_empty())
                .is_some_and(c_family_decorated_type_line)
                && text.contains('{')
                && text.trim_end().ends_with("};")
        })
}

fn recoverable_preprocessor_error(
    content: &str,
    mut node: Node<'_>,
    range: &nodes::SyntaxRange,
) -> bool {
    let line_starts_with_directive = source_line(content, range.line_start)
        .is_some_and(|line| line.trim_start().starts_with('#'));
    loop {
        if node.kind().starts_with("preproc") {
            if matches!(
                node.kind(),
                "preproc_def" | "preproc_function_def" | "preproc_include" | "preproc_call"
            ) {
                let preprocessor_range = nodes::syntax_range(node);
                return preprocessor_range
                    .line_end
                    .saturating_sub(preprocessor_range.line_start)
                    <= 2;
            }
            return line_starts_with_directive;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn source_line(content: &str, line_number: usize) -> Option<&str> {
    line_number
        .checked_sub(1)
        .and_then(|index| content.lines().nth(index))
}

pub(super) fn recoverable_c_family_error_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    if (trimmed.starts_with("template class ") || trimmed.starts_with("template struct "))
        && trimmed.contains('<')
        && trimmed.contains('>')
        && trimmed.ends_with(';')
    {
        return true;
    }
    if c_family_decorated_type_line(trimmed) {
        return true;
    }
    if c_family_typedef_like_error_line(trimmed) {
        return true;
    }

    let Some(token) = trimmed
        .split(|character: char| !c_identifier_char(character))
        .next()
    else {
        return false;
    };
    c_family_macro_name(token) && trimmed.contains('(')
}

fn c_family_typedef_like_error_line(trimmed: &str) -> bool {
    if trimmed.contains("=;") || trimmed.contains("= ;") {
        return false;
    }

    c_family_recoverable_error_function_signature(trimmed)
        || c_family_typedef_like_initializer_declaration(trimmed)
}
