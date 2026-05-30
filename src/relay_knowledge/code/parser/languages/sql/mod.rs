use tree_sitter::Node;

use super::super::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "create_function" => Some("function"),
        "create_procedure" => Some("function"),
        "create_materialized_view" => Some("view"),
        "create_table" => Some("table"),
        "create_trigger" => Some("trigger"),
        "create_type" => Some("type"),
        "create_view" => Some("view"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "invocation"
}

pub(in crate::code::parser) fn manual_definition_candidate(node_kind: &str) -> bool {
    node_kind == "ERROR" || definition_kind(node_kind).is_some()
}

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if let Some(procedure) = recovered_create_procedure_definition(content, node) {
        return vec![procedure];
    }

    let Some(kind) = definition_kind(node.kind()) else {
        return Vec::new();
    };
    let Some(name_node) = first_child_of_kind(node, "object_reference") else {
        return Vec::new();
    };
    let Some(name) = object_reference_name(content, name_node) else {
        return Vec::new();
    };

    vec![(name, kind, syntax_range(node))]
}

pub(in crate::code::parser) fn manual_call(
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    if node.kind() != "invocation" {
        return None;
    }
    let target = first_child_of_kind(node, "object_reference")?;

    Some((
        object_reference_name(content, target)?,
        syntax_range(target),
    ))
}

pub(in crate::code::parser) fn manual_reference(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if node.kind() != "object_reference" || object_reference_is_definition_target(node) {
        return None;
    }
    if node
        .parent()
        .is_some_and(|parent| matches!(parent.kind(), "field" | "invocation"))
    {
        return None;
    }

    let kind = if object_reference_is_trigger_function_call(node) {
        "call"
    } else {
        "reference"
    };
    Some((
        object_reference_name(content, node)?,
        kind,
        syntax_range(node),
    ))
}

fn recovered_create_procedure_definition(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if node.kind() != "ERROR" {
        return None;
    }

    let text = node_text(content, node);
    let after_create = strip_keyword_prefix(&text, "create")?;
    let after_replace = strip_keyword_prefix(after_create, "or")
        .and_then(|after_or| strip_keyword_prefix(after_or, "replace"))
        .unwrap_or(after_create);
    let after_procedure = strip_keyword_prefix(after_replace, "procedure")?;
    let (name, remainder) = parse_qualified_identifier(after_procedure)?;
    if !remainder.trim_start().starts_with('(') {
        return None;
    }

    Some((name, "function", syntax_range(node)))
}

fn object_reference_is_definition_target(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !object_reference_parent_declares_target(parent.kind()) {
        return false;
    }

    first_child_of_kind(parent, "object_reference").is_some_and(|target| same_node(target, node))
}

fn object_reference_parent_declares_target(parent_kind: &str) -> bool {
    definition_kind(parent_kind).is_some() || parent_kind == "create_sequence"
}

fn object_reference_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut parts = Vec::new();
    for field_name in ["database", "schema", "name"] {
        if let Some(part) = node
            .child_by_field_name(field_name)
            .map(|child| normalize_identifier_component(&node_text(content, child)))
            .filter(|part| !part.is_empty())
        {
            parts.push(part);
        }
    }

    (!parts.is_empty()).then(|| parts.join("."))
}

fn object_reference_is_trigger_function_call(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "create_trigger")
        && node.prev_named_sibling().is_some_and(|sibling| {
            matches!(sibling.kind(), "keyword_function" | "keyword_procedure")
        })
}

fn parse_qualified_identifier(input: &str) -> Option<(String, &str)> {
    let mut position = skip_ascii_whitespace(input, 0);
    let mut parts = Vec::new();

    loop {
        let (part, next_position) = parse_identifier_component(input, position)?;
        parts.push(part);
        let dot_position = skip_ascii_whitespace(input, next_position);
        if !input.get(dot_position..)?.starts_with('.') {
            return Some((parts.join("."), &input[next_position..]));
        }
        position = skip_ascii_whitespace(input, dot_position + 1);
    }
}

fn parse_identifier_component(input: &str, position: usize) -> Option<(String, usize)> {
    let remaining = input.get(position..)?;
    let first = remaining.chars().next()?;
    if quoted_identifier_start(first) {
        let end = scan_quoted_identifier(input, position, first)?;
        return Some((
            normalize_identifier_component(input.get(position..end)?),
            end,
        ));
    }
    if !unquoted_identifier_start(first) {
        return None;
    }

    let end = input
        .get(position..)?
        .char_indices()
        .find_map(|(offset, value)| (!unquoted_identifier_char(value)).then_some(position + offset))
        .unwrap_or(input.len());
    Some((
        normalize_identifier_component(input.get(position..end)?),
        end,
    ))
}

fn scan_quoted_identifier(input: &str, position: usize, start: char) -> Option<usize> {
    let end = match start {
        '"' => '"',
        '`' => '`',
        '[' => ']',
        _ => return None,
    };
    let mut cursor = position + start.len_utf8();
    while let Some(rest) = input.get(cursor..) {
        let current = rest.chars().next()?;
        cursor += current.len_utf8();
        if current == end {
            if input
                .get(cursor..)
                .is_some_and(|next| next.starts_with(end))
            {
                cursor += end.len_utf8();
                continue;
            }
            return Some(cursor);
        }
    }

    None
}

fn normalize_identifier_component(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(unquoted) = unquote_delimited_identifier(trimmed) {
        return unquoted;
    }

    trimmed.to_ascii_lowercase()
}

fn unquote_delimited_identifier(value: &str) -> Option<String> {
    let (open, close) = match value.chars().next()? {
        '"' => ('"', '"'),
        '`' => ('`', '`'),
        '[' => ('[', ']'),
        _ => return None,
    };
    let inner = value.strip_prefix(open)?.strip_suffix(close)?;
    let escaped_close = format!("{close}{close}");
    Some(inner.replace(&escaped_close, &close.to_string()))
}

fn strip_keyword_prefix<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = input.trim_start();
    let prefix = trimmed.get(..keyword.len())?;
    if !prefix.eq_ignore_ascii_case(keyword) {
        return None;
    }
    let remainder = trimmed.get(keyword.len()..)?;
    if remainder
        .chars()
        .next()
        .is_some_and(unquoted_identifier_char)
    {
        return None;
    }

    Some(remainder)
}

fn skip_ascii_whitespace(input: &str, position: usize) -> usize {
    input
        .get(position..)
        .and_then(|rest| {
            rest.char_indices()
                .find_map(|(offset, value)| (!value.is_ascii_whitespace()).then_some(offset))
        })
        .map(|offset| position + offset)
        .unwrap_or(input.len())
}

fn quoted_identifier_start(value: char) -> bool {
    matches!(value, '"' | '`' | '[')
}

fn unquoted_identifier_start(value: char) -> bool {
    value == '_' || value.is_ascii_alphabetic()
}

fn unquoted_identifier_char(value: char) -> bool {
    matches!(value, '_' | '$') || value.is_ascii_alphanumeric()
}

fn first_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..node.child_count()).find_map(|index| {
        let child = node.child(u32::try_from(index).ok()?)?;
        (child.kind() == kind).then_some(child)
    })
}

fn same_node(left: Node<'_>, right: Node<'_>) -> bool {
    left.start_byte() == right.start_byte() && left.end_byte() == right.end_byte()
}
