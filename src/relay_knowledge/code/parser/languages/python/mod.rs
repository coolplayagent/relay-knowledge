//! Python tree-sitter reference classification.

mod annotations;
mod node_kinds;

use tree_sitter::Node;

use super::super::super::languages::python_builtin_type_reference;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) use annotations::manual_type_references;
pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};

const MAX_TYPEVAR_LOOKBACK_LINES: usize = 4096;

pub(in crate::code::parser) fn manual_reference(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if !python_node_in_type_reference(node) {
        return Vec::new();
    }
    let range = syntax_range(node);
    python_type_reference_names(content, node)
        .into_iter()
        .filter(|name| python_type_identifier_reference(name))
        .filter(|name| !python_local_typevar_reference(content, node, name))
        .map(|name| (name, "type", range.clone()))
        .collect()
}

fn python_type_reference_names(content: &str, node: Node<'_>) -> Vec<String> {
    match node.kind() {
        "identifier" => vec![node_text(content, node)],
        "string" if !python_string_literal_type_argument(content, node) => {
            quoted_python_type_reference_names(&node_text(content, node))
        }
        _ => Vec::new(),
    }
}

fn quoted_python_type_reference_names(text: &str) -> Vec<String> {
    let text = text.trim();
    let text = text.trim_start_matches(['r', 'R', 'u', 'U']);
    for quote in ['"', '\''] {
        let Some(inner) = text
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
        else {
            continue;
        };
        return python_type_expression_identifiers(inner);
    }

    Vec::new()
}

fn python_type_expression_identifiers(expression: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in expression.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            if let Some(start_index) = start.take() {
                identifiers.push(expression[start_index..index].to_owned());
            }
            quote = Some(character);
            escaped = false;
            continue;
        }
        if let Some(start_index) = start {
            if python_identifier_continue(character) {
                continue;
            }
            identifiers.push(expression[start_index..index].to_owned());
            start = python_identifier_start(character).then_some(index);
        } else if python_identifier_start(character) {
            start = Some(index);
        }
    }
    if let Some(start_index) = start {
        identifiers.push(expression[start_index..].to_owned());
    }

    identifiers
}

fn python_identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn python_identifier_continue(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn python_string_literal_type_argument(content: &str, node: Node<'_>) -> bool {
    let mut current = node;
    for _ in 0..6 {
        let Some(parent) = current.parent() else {
            return false;
        };
        if parent.kind() == "generic_type" && generic_type_base_matches(content, parent, "Literal")
        {
            return true;
        }
        current = parent;
    }

    false
}

fn generic_type_base_matches(content: &str, node: Node<'_>, expected: &str) -> bool {
    let text = node_text(content, node);
    let base = text.split(['[', '(']).next().unwrap_or_default().trim_end();
    base.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|token| !token.is_empty())
        .is_some_and(|token| token == expected)
}

fn python_local_typevar_reference(content: &str, node: Node<'_>, name: &str) -> bool {
    if python_local_type_parameter_reference(content, node, name) {
        return true;
    }
    let Some(prefix) = content.get(..node.start_byte()) else {
        return false;
    };
    prefix
        .lines()
        .rev()
        .take(MAX_TYPEVAR_LOOKBACK_LINES)
        .any(|line| python_typevar_definition_line(line, name))
}

fn python_typevar_definition_line(line: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    let Some(rest) = trimmed.strip_prefix(name) else {
        return false;
    };
    if rest.chars().next().is_some_and(python_identifier_continue) {
        return false;
    }
    let rest = rest.trim_start();
    let assignment = if let Some(rest) = rest.strip_prefix(':') {
        let Some((_, assignment)) = rest.split_once('=') else {
            return false;
        };
        assignment
    } else {
        let Some(assignment) = rest.strip_prefix('=') else {
            return false;
        };
        assignment
    };
    let assignment = assignment.trim_start();
    assignment.starts_with("TypeVar(")
        || assignment.starts_with("typing.TypeVar(")
        || assignment.starts_with("TypeVarTuple(")
        || assignment.starts_with("typing.TypeVarTuple(")
        || assignment.starts_with("ParamSpec(")
        || assignment.starts_with("typing.ParamSpec(")
}

fn python_local_type_parameter_reference(content: &str, node: Node<'_>, name: &str) -> bool {
    let mut current = node;
    for _ in 0..12 {
        let Some(parent) = current.parent() else {
            return false;
        };
        if type_parameters_node(parent).is_some_and(|type_parameters| {
            !node_contains(type_parameters, node)
                && type_parameters_contain_name(content, type_parameters, name)
        }) {
            return true;
        }
        current = parent;
    }

    false
}

fn type_parameters_node(parent: Node<'_>) -> Option<Node<'_>> {
    parent.child_by_field_name("type_parameters").or_else(|| {
        let mut cursor = parent.walk();
        parent
            .children(&mut cursor)
            .find(|child| child.kind() == "type_parameters")
    })
}

fn type_parameters_contain_name(content: &str, type_parameters: Node<'_>, name: &str) -> bool {
    if type_parameters.kind() == "type_parameter" {
        return type_parameter_name(content, type_parameters)
            .is_some_and(|parameter_name| parameter_name == name);
    }
    let mut cursor = type_parameters.walk();
    type_parameters.children(&mut cursor).any(|child| {
        if child.kind() == "type_parameter" {
            return type_parameter_name(content, child)
                .is_some_and(|parameter_name| parameter_name == name);
        }
        child.kind() == "identifier" && node_text(content, child) == name
    })
}

fn type_parameter_name(content: &str, type_parameter: Node<'_>) -> Option<String> {
    type_parameter
        .child_by_field_name("name")
        .map(|name| node_text(content, name))
        .or_else(|| first_identifier_name(content, type_parameter))
}

fn first_identifier_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "identifier" {
            return Some(node_text(content, current));
        }
        let mut cursor = current.walk();
        let children = current.children(&mut cursor).collect::<Vec<_>>();
        stack.extend(children.into_iter().rev());
    }

    None
}

fn python_node_in_type_reference(node: Node<'_>) -> bool {
    if node_is_definition_name(node) {
        return false;
    }
    let mut current = node;
    for _ in 0..6 {
        let Some(parent) = current.parent() else {
            return false;
        };
        if field_contains_node(parent, node, "type")
            || field_contains_node(parent, node, "return_type")
        {
            return true;
        }
        if !python_type_context_node(parent.kind()) {
            return false;
        }
        current = parent;
    }

    false
}

fn node_is_definition_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(parent.kind(), "class_definition" | "function_definition")
            && field_contains_node(parent, node, "name")
    })
}

fn python_type_context_node(kind: &str) -> bool {
    matches!(
        kind,
        "type"
            | "generic_type"
            | "member_type"
            | "union_type"
            | "typed_parameter"
            | "parameters"
            | "return_type"
            | "type_parameter"
            | "subscript"
            | "list"
            | "tuple"
    )
}

fn python_type_identifier_reference(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_uppercase())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        && !python_builtin_type_reference(name)
}

fn field_contains_node(parent: Node<'_>, target: Node<'_>, field: &str) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|child| node_contains(child, target))
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}
