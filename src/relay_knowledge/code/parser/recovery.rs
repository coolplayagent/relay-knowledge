use super::nodes::{self, push_children_reverse};
use tree_sitter::Node;
pub(in crate::code::parser) mod scan;
mod type_body;
use scan::{
    CodeScanState, first_code_char_index, line_has_balanced_delimiters,
    scan_code_line_indices_with_state,
};
pub(super) use scan::{
    code_contains_char, parameter_list_has_empty_slot, scan_code_line_indices,
    token_starts_in_angle_arguments,
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

pub(in crate::code::parser) fn decorated_function_head_has_recoverable_tail(
    head: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    c_family_function_signature_tail_is_recoverable(
        head,
        allow_default_arguments,
        allow_cpp_method_suffix,
        allow_operator_declarator,
    )
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
    !line.chars().any(|character| matches!(character, '@' | '`'))
        && (line.starts_with('#')
            || line.ends_with(';')
            || line.ends_with('{')
            || line.ends_with('}')
            || line.starts_with('}')
            || c_family_statement_label_line(line))
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

    c_family_typedef_like_function_signature(trimmed)
        || c_family_typedef_like_initializer_declaration(trimmed)
}

pub(in crate::code::parser) fn c_family_typedef_like_function_signature(trimmed: &str) -> bool {
    c_family_typedef_like_function_signature_with_options(trimmed, false, true, false)
}

fn c_family_typedef_like_function_signature_with_options(
    trimmed: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    if !line_has_balanced_delimiters(trimmed) {
        return false;
    }
    let Some(parameter_start) =
        c_family_top_level_parameter_start(trimmed, allow_operator_declarator)
    else {
        return false;
    };
    let head = &trimmed[..parameter_start];
    if code_contains_char(head, '=') && !c_family_operator_before_open(trimmed, parameter_start) {
        return false;
    }
    if !c_family_function_signature_tail_is_recoverable(
        trimmed,
        allow_default_arguments,
        allow_cpp_method_suffix,
        allow_operator_declarator,
    ) {
        return false;
    }

    c_family_typedef_declaration_head(head)
}

fn c_family_function_signature_tail_is_recoverable(
    trimmed: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    if !line_has_balanced_delimiters(trimmed) {
        return false;
    }
    let Some(parameter_start) =
        c_family_top_level_parameter_start(trimmed, allow_operator_declarator)
    else {
        return false;
    };
    let Some(parameter_end) = c_family_closing_parenthesis_index(&trimmed[parameter_start..])
    else {
        return false;
    };
    let parameter_text = &trimmed[parameter_start + 1..parameter_start + parameter_end];
    if parameter_list_has_empty_slot(parameter_text) {
        return false;
    }
    if !allow_default_arguments && code_contains_char(parameter_text, '=') {
        return false;
    }
    let tail = trimmed[parameter_start + parameter_end + 1..].trim();
    c_family_typedef_signature_tail_is_declaration_shaped(tail, allow_cpp_method_suffix)
}

fn c_family_top_level_parameter_start(
    text: &str,
    allow_operator_declarator: bool,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut candidate = None;
    let literals_closed = scan_code_line_indices(text, |index, character| match character {
        '(' => {
            if depth == 0
                && c_family_parameter_open_looks_like_declarator(
                    text,
                    index,
                    allow_operator_declarator,
                )
            {
                candidate = Some(index);
            }
            depth += 1;
        }
        ')' => depth = depth.saturating_sub(1),
        _ => {}
    });

    literals_closed.then_some(candidate).flatten()
}

fn c_family_parameter_open_looks_like_declarator(
    text: &str,
    parameter_start: usize,
    allow_operator_declarator: bool,
) -> bool {
    c_identifier_before_open(text, parameter_start).is_some_and(|token| {
        c_identifier_name(token)
            && !c_declaration_qualifier_token(token)
            && !c_family_known_decorator_token(token)
            && !c_family_decorator_payload_token(token)
    }) || (allow_operator_declarator && c_family_operator_before_open(text, parameter_start))
}

fn c_identifier_before_open(text: &str, parameter_start: usize) -> Option<&str> {
    let name_end = text[..parameter_start].trim_end().len();
    let name_start = text[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some(&text[name_start..name_end])
}

fn c_family_operator_before_open(text: &str, parameter_start: usize) -> bool {
    let prefix = &text[..text[..parameter_start].trim_end().len()];
    let Some(operator_start) = prefix.rfind("operator") else {
        return false;
    };
    if prefix[..operator_start]
        .chars()
        .next_back()
        .is_some_and(c_identifier_char)
    {
        return false;
    }
    let suffix = prefix[operator_start + "operator".len()..].trim();
    c_family_punctuation_operator_suffix(suffix) || c_family_conversion_operator_suffix(suffix)
}

fn c_family_punctuation_operator_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .all(|character| character.is_ascii_punctuation() || character.is_ascii_whitespace())
}

fn c_family_conversion_operator_suffix(suffix: &str) -> bool {
    let tokens = c_family_head_tokens(suffix);
    !tokens.is_empty()
        && tokens.iter().all(|token| {
            c_declaration_qualifier_token(token.text)
                || c_identifier_name(token.text)
                || c_family_builtin_type_token(token.text)
        })
        && suffix.chars().all(|character| {
            c_identifier_char(character)
                || character.is_ascii_whitespace()
                || matches!(character, ':' | '*' | '&' | '<' | '>' | ',')
        })
}

fn c_family_typedef_signature_tail_is_declaration_shaped(
    tail: &str,
    allow_cpp_method_suffix: bool,
) -> bool {
    tail.is_empty()
        || matches!(tail, ";" | "{")
        || (allow_cpp_method_suffix && c_family_cpp_method_suffix_tail(tail))
        || c_family_postfix_attribute_tail(tail)
}

pub(in crate::code::parser) fn decorated_function_head_has_recovery_decorator(head: &str) -> bool {
    let Some(parameter_start) = c_family_top_level_parameter_start(head, true) else {
        return false;
    };
    let parameter_end = c_family_closing_parenthesis_index(&head[parameter_start..])
        .map_or(head.len(), |index| parameter_start + index + 1);
    let prefix = &head[..parameter_start];
    let suffix = &head[parameter_end..];
    c_family_head_tokens(prefix)
        .iter()
        .chain(c_family_head_tokens(suffix).iter())
        .any(|token| {
            c_family_known_decorator_token(token.text)
                || matches!(
                    token.text,
                    "__always_inline"
                        | "__inline"
                        | "__inline__"
                        | "__declspec"
                        | "__declspec__"
                        | "__attribute"
                        | "__attribute__"
                        | "attribute"
                )
        })
}

fn c_family_postfix_attribute_tail(mut tail: &str) -> bool {
    let mut consumed_attribute = false;
    loop {
        tail = tail.trim_start();
        if tail.is_empty() || matches!(tail, ";" | "{") {
            return consumed_attribute;
        }
        let Some((token, token_end)) = c_family_leading_identifier(tail) else {
            return false;
        };
        if c_family_known_decorator_token(token) {
            let after_token = tail[token_end..].trim_start();
            let Some(payload_end) = c_family_parenthesized_prefix_end(after_token) else {
                return false;
            };
            tail = &after_token[payload_end..];
            consumed_attribute = true;
            continue;
        }
        if matches!(
            token,
            "const" | "final" | "noexcept" | "override" | "volatile"
        ) {
            tail = &tail[token_end..];
            if token == "noexcept" {
                let trimmed = tail.trim_start();
                if let Some(payload_end) = c_family_parenthesized_prefix_end(trimmed) {
                    tail = &trimmed[payload_end..];
                }
            }
            continue;
        }
        return false;
    }
}

fn c_family_leading_identifier(text: &str) -> Option<(&str, usize)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, character) in chars {
        if !c_identifier_char(character) {
            return Some((&text[..end], index));
        }
        end = index + character.len_utf8();
    }
    Some((text, text.len()))
}

fn c_family_parenthesized_prefix_end(text: &str) -> Option<usize> {
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
                    matched_end = Some(index + character.len_utf8());
                }
            }
            _ => {}
        }
    });
    literals_closed.then_some(matched_end).flatten()
}

fn c_family_cpp_method_suffix_tail(tail: &str) -> bool {
    let mut tail = tail.trim();
    if let Some(stripped) = tail.strip_suffix("= 0;") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("=0;") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("= 0") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("=0") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix('{').or_else(|| tail.strip_suffix(';')) {
        tail = stripped.trim_end();
    }
    if tail.is_empty() || !line_has_balanced_delimiters(tail) {
        return false;
    }
    let mut consumed_suffix = false;
    loop {
        tail = tail.trim_start();
        if tail.is_empty() {
            return consumed_suffix;
        }
        let Some((token, token_end)) = c_family_leading_identifier(tail) else {
            return false;
        };
        if !matches!(
            token,
            "const" | "final" | "noexcept" | "override" | "volatile"
        ) {
            return false;
        }
        tail = &tail[token_end..];
        if token == "noexcept" {
            let trimmed = tail.trim_start();
            if let Some(payload_end) = c_family_parenthesized_prefix_end(trimmed) {
                tail = &trimmed[payload_end..];
            }
        }
        consumed_suffix = true;
    }
}

fn c_family_closing_parenthesis_index(text: &str) -> Option<usize> {
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

fn c_family_typedef_like_initializer_declaration(trimmed: &str) -> bool {
    let Some((head, initializer)) = trimmed.split_once('=') else {
        return false;
    };
    let initializer = initializer.trim_start();
    if !initializer.starts_with('{') {
        return false;
    }

    c_family_typedef_declaration_head(head)
}

fn c_family_typedef_declaration_head(head: &str) -> bool {
    let normalized;
    let head = if let Some(stripped) = c_family_head_without_declarator_scope(head) {
        normalized = stripped;
        normalized.as_str()
    } else {
        head
    };
    let tokens = c_family_head_tokens(head);
    let name = tokens
        .last()
        .copied()
        .filter(|token| c_identifier_name(token.text));
    let Some(name) = name else {
        return false;
    };
    if c_family_builtin_type_token(name.text)
        || (name.text.ends_with("_t") && c_identifier_name(name.text))
    {
        return false;
    }
    let Some(type_index) = tokens[..tokens.len().saturating_sub(1)]
        .iter()
        .rposition(|token| {
            !c_declaration_qualifier_token(token.text)
                && !token_starts_in_angle_arguments(head, token.start)
        })
    else {
        return false;
    };
    if !c_family_typedef_like_type_at(head, &tokens, type_index) {
        return false;
    }
    let type_start = c_family_qualified_type_start(head, &tokens, type_index);
    if !tokens[..type_start].iter().all(|token| {
        c_declaration_qualifier_token(token.text)
            || c_family_typedef_like_type_token(token.text)
            || c_family_decorator_payload_token(token.text)
    }) {
        return false;
    }

    true
}

fn c_family_head_without_declarator_scope(head: &str) -> Option<String> {
    if let Some(operator_start) = head.rfind("operator") {
        let stripped_prefix =
            c_family_strip_trailing_qualified_declarator_scope(&head[..operator_start]);
        return (stripped_prefix.len() < operator_start)
            .then(|| format!("{stripped_prefix}operator"));
    }
    let name_end = head.trim_end().len();
    let name_start = head[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    if name_start >= name_end {
        return None;
    }
    let prefix = &head[..name_start];
    let stripped_prefix = c_family_strip_trailing_qualified_declarator_scope(prefix);
    (stripped_prefix.len() < prefix.len())
        .then(|| format!("{stripped_prefix}{}", &head[name_start..name_end]))
}

#[derive(Clone, Copy)]
struct CFamilyHeadToken<'text> {
    text: &'text str,
    start: usize,
    end: usize,
}

fn c_family_head_tokens(head: &str) -> Vec<CFamilyHeadToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, character) in head.char_indices() {
        if c_identifier_char(character) {
            token_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = token_start.take() {
            tokens.push(CFamilyHeadToken {
                text: &head[start..index],
                start,
                end: index,
            });
        }
    }
    if let Some(start) = token_start {
        tokens.push(CFamilyHeadToken {
            text: &head[start..],
            start,
            end: head.len(),
        });
    }

    tokens
}

fn c_family_strip_trailing_qualified_declarator_scope(prefix: &str) -> &str {
    let mut cursor = prefix.trim_end().len();
    let mut stripped_scope = false;
    loop {
        let before_colons = prefix[..cursor].trim_end().len();
        if !prefix[..before_colons].ends_with("::") {
            break;
        }
        let scope_end = before_colons.saturating_sub(2);
        let ident_end = prefix[..scope_end].trim_end().len();
        let ident_start = prefix[..ident_end]
            .char_indices()
            .rev()
            .find(|(_, character)| !c_identifier_char(*character))
            .map_or(0, |(index, character)| index + character.len_utf8());
        if ident_start >= ident_end {
            break;
        }
        cursor = ident_start;
        stripped_scope = true;
    }

    if stripped_scope {
        &prefix[..cursor]
    } else {
        prefix
    }
}

fn c_family_typedef_like_type_at(
    head: &str,
    tokens: &[CFamilyHeadToken<'_>],
    type_index: usize,
) -> bool {
    let token = tokens[type_index].text;
    if c_family_typedef_like_type_token(token) {
        return true;
    }
    c_family_qualified_type_start(head, tokens, type_index) < type_index
        && c_identifier_name(token)
        && !c_family_builtin_type_token(token)
}

fn c_family_qualified_type_start(
    head: &str,
    tokens: &[CFamilyHeadToken<'_>],
    type_index: usize,
) -> usize {
    let mut start = type_index;
    while start > 0
        && c_identifier_name(tokens[start - 1].text)
        && c_family_tokens_joined_by_qualifier(head, tokens[start - 1], tokens[start])
    {
        start -= 1;
    }
    start
}

fn c_family_tokens_joined_by_qualifier(
    head: &str,
    left: CFamilyHeadToken<'_>,
    right: CFamilyHeadToken<'_>,
) -> bool {
    let separator = &head[left.end..right.start];
    separator.contains("::")
        && separator
            .chars()
            .all(|character| character == ':' || character.is_ascii_whitespace())
}

fn c_family_typedef_like_type_token(token: &str) -> bool {
    c_family_builtin_type_token(token)
        || c_family_tag_type_keyword(token)
        || (token.ends_with("_t") && c_identifier_name(token))
        || c_family_external_type_token(token)
}

fn c_family_builtin_type_token(token: &str) -> bool {
    matches!(
        token,
        "bool"
            | "char"
            | "double"
            | "float"
            | "int"
            | "long"
            | "short"
            | "signed"
            | "unsigned"
            | "void"
    )
}

fn c_family_tag_type_keyword(token: &str) -> bool {
    matches!(token, "enum" | "struct" | "union")
}

fn c_family_external_type_token(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        && token
            .chars()
            .any(|character| character.is_ascii_lowercase())
        && c_identifier_name(token)
}

fn c_declaration_qualifier_token(token: &str) -> bool {
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
            | "const"
            | "extern"
            | "inline"
            | "register"
            | "restrict"
            | "static"
            | "volatile"
    )
}

fn c_family_decorated_type_line(trimmed: &str) -> bool {
    let tokens = trimmed
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        matches!(*token, "class" | "struct" | "enum" | "union")
            && (c_family_decorator_before_type(&tokens, index)
                || c_family_decorator_after_type(&tokens, index))
    })
}

fn c_family_decorator_before_type(tokens: &[&str], index: usize) -> bool {
    let mut cursor = index;
    while let Some(previous_index) = cursor.checked_sub(1) {
        let Some(previous) = tokens.get(previous_index) else {
            return false;
        };
        if c_family_known_decorator_token(previous) {
            return true;
        }
        if !c_family_decorator_payload_token(previous) {
            return false;
        }
        cursor = previous_index;
    }

    false
}

fn c_family_decorator_after_type(tokens: &[&str], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|candidate| c_family_known_decorator_token(candidate))
}

fn c_family_known_decorator_token(token: &str) -> bool {
    matches!(
        token,
        "__attribute__" | "__attribute" | "__declspec" | "__declspec__" | "attribute"
    ) || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
}

fn c_family_decorator_payload_token(token: &str) -> bool {
    matches!(
        token,
        "always_inline"
            | "annotate"
            | "dllimport"
            | "dllexport"
            | "visibility"
            | "default"
            | "hidden"
    )
}

fn c_family_macro_name(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
        && token.chars().any(|character| character == '_')
        && token
            .chars()
            .any(|character| character.is_ascii_uppercase())
}

fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn c_identifier_name(token: &str) -> bool {
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(c_identifier_char)
}
