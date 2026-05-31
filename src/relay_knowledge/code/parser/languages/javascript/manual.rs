use tree_sitter::Node;

use super::super::super::super::languages::typescript_builtin_type_reference;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};

const MAX_EXPORTED_VALUE_LINES: usize = 64;
const MAX_FUNCTION_FACTORY_CALL_DEPTH: usize = 4;

pub(in crate::code::parser) fn manual_definition_candidate(node_kind: &str) -> bool {
    matches!(
        node_kind,
        "assignment_expression" | "pair" | "public_field_definition" | "variable_declarator"
    )
}

pub(in crate::code::parser) fn manual_definition(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    javascript_like_function_value_definition(content, node)
        .or_else(|| javascript_like_exported_value_definition(content, node))
}

pub(in crate::code::parser) fn manual_reference(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(language_id, "typescript" | "tsx")
        || !matches!(node.kind(), "identifier" | "type_identifier")
    {
        return None;
    }
    let name = node_text(content, node);
    if !typescript_identifier_in_type_reference(content, node, &name) {
        return None;
    }
    javascript_type_identifier_reference(&name).then(|| (name, "type", syntax_range(node)))
}

pub(in crate::code::parser) fn exported_declaration_range(node: Node<'_>) -> Option<SyntaxRange> {
    if !matches!(
        node.kind(),
        "class_declaration"
            | "enum_declaration"
            | "function_declaration"
            | "generator_function_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
    ) {
        return None;
    }

    export_statement_ancestor(node).map(syntax_range)
}

fn typescript_identifier_in_type_reference(content: &str, node: Node<'_>, name: &str) -> bool {
    if node_is_definition_name(node) || node_is_local_type_parameter_reference(content, node, name)
    {
        return false;
    }
    let mut current = node;
    for _ in 0..8 {
        let Some(parent) = current.parent() else {
            return false;
        };
        if field_contains_node(parent, node, "type")
            || field_contains_node(parent, node, "return_type")
            || field_contains_node(parent, node, "constraint")
        {
            return true;
        }
        if !typescript_type_context_node(parent.kind()) {
            return false;
        }
        current = parent;
    }

    false
}

fn node_is_local_type_parameter_reference(content: &str, node: Node<'_>, name: &str) -> bool {
    if line_prefix_declares_type_parameter(content, node, name) {
        return true;
    }

    let mut current = node;
    for _ in 0..10 {
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

fn line_prefix_declares_type_parameter(content: &str, node: Node<'_>, name: &str) -> bool {
    let Some(prefix) = content.get(..node.start_byte()) else {
        return false;
    };
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    closed_type_parameter_segments(prefix.get(line_start..).unwrap_or_default())
        .any(|segment| type_parameter_segment_contains_name(segment, name))
}

fn closed_type_parameter_segments(line_prefix: &str) -> impl Iterator<Item = &str> {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        let start = line_prefix.get(cursor..)?.find('<')? + cursor;
        let end = line_prefix.get(start + 1..)?.find('>')? + start + 1;
        let after = line_prefix.get(end + 1..).unwrap_or_default().trim_start();
        cursor = end + 1;
        after
            .starts_with('(')
            .then(|| line_prefix.get(start + 1..end))
            .flatten()
    })
}

fn type_parameter_segment_contains_name(segment: &str, name: &str) -> bool {
    segment.split(',').any(|parameter| {
        parameter
            .trim()
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .find(|token| !token.is_empty())
            .is_some_and(|parameter_name| parameter_name == name)
    })
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
    let mut cursor = type_parameters.walk();
    type_parameters.children(&mut cursor).any(|child| {
        if child.kind() == "type_parameter" {
            return type_parameter_name(content, child)
                .is_some_and(|parameter_name| parameter_name == name);
        }
        matches!(child.kind(), "identifier" | "type_identifier")
            && node_text(content, child) == name
    })
}

fn type_parameter_name(content: &str, type_parameter: Node<'_>) -> Option<String> {
    type_parameter
        .child_by_field_name("name")
        .map(|name| node_text(content, name))
        .or_else(|| {
            let mut cursor = type_parameter.walk();
            type_parameter
                .children(&mut cursor)
                .find(|child| matches!(child.kind(), "identifier" | "type_identifier"))
                .map(|name| node_text(content, name))
        })
}

fn node_is_definition_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "class_declaration"
                | "enum_declaration"
                | "function_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
        ) && field_contains_node(parent, node, "name")
    })
}

fn typescript_type_context_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_type"
            | "conditional_type"
            | "constraint"
            | "generic_type"
            | "intersection_type"
            | "lookup_type"
            | "nested_type_identifier"
            | "object_type"
            | "optional_type"
            | "parenthesized_type"
            | "predefined_type"
            | "readonly_type"
            | "return_type"
            | "tuple_type"
            | "type_annotation"
            | "type_arguments"
            | "type_predicate"
            | "union_type"
    )
}

fn javascript_type_identifier_reference(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_uppercase())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        && !typescript_builtin_type_reference(name)
}

fn field_contains_node(parent: Node<'_>, target: Node<'_>, field: &str) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|child| node_contains(child, target))
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}

fn javascript_like_function_value_definition(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !manual_definition_candidate(node.kind()) {
        return None;
    }
    let value = function_value_node(node)?;
    if !javascript_like_function_value(node, value) {
        return None;
    }
    let name = function_value_name(content, node)?;
    if !javascript_identifier_name(&name) {
        return None;
    }

    Some((name, "function", javascript_like_function_value_range(node)))
}

fn javascript_like_function_value(owner: Node<'_>, value: Node<'_>) -> bool {
    if javascript_like_function_node(value) {
        return true;
    }
    matches!(owner.kind(), "pair" | "public_field_definition")
        && javascript_like_function_factory_call(value, 0)
}

fn javascript_like_function_factory_call(value: Node<'_>, depth: usize) -> bool {
    if depth >= MAX_FUNCTION_FACTORY_CALL_DEPTH || value.kind() != "call_expression" {
        return false;
    }
    let curried_factory = value
        .child_by_field_name("function")
        .is_some_and(|function| function.kind() == "call_expression");
    if !curried_factory {
        return false;
    }
    if value
        .child_by_field_name("arguments")
        .is_some_and(arguments_include_function_node)
    {
        return true;
    }
    value
        .child_by_field_name("function")
        .is_some_and(|function| javascript_like_function_factory_call(function, depth + 1))
}

fn arguments_include_function_node(arguments: Node<'_>) -> bool {
    (0..arguments.child_count()).any(|index| {
        let Ok(index) = u32::try_from(index) else {
            return false;
        };
        arguments
            .child(index)
            .is_some_and(javascript_like_function_node)
    })
}

fn javascript_like_function_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    )
}

fn javascript_like_function_value_range(node: Node<'_>) -> SyntaxRange {
    exported_variable_declaration(node)
        .map(syntax_range)
        .unwrap_or_else(|| syntax_range(node))
}

fn exported_variable_declaration(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() != "variable_declarator" {
        return None;
    }
    let declaration = node.parent()?;
    if !matches!(
        declaration.kind(),
        "lexical_declaration" | "variable_declaration"
    ) {
        return None;
    }
    declaration
        .parent()
        .filter(|parent| parent.kind() == "export_statement")
}

fn javascript_like_exported_value_definition(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let export_statement = export_statement_ancestor(node);
    if node.kind() != "variable_declarator" || export_statement.is_none() {
        return None;
    }
    let value = node.child_by_field_name("value")?;
    if !javascript_like_retrievable_exported_value(value) {
        return None;
    }
    let name = named_property_text(content, node.child_by_field_name("name")?)?;
    if !javascript_identifier_name(&name) {
        return None;
    }
    let range = syntax_range(node);
    if range.line_end.saturating_sub(range.line_start) > MAX_EXPORTED_VALUE_LINES {
        return None;
    }

    let range = export_statement.map(syntax_range).unwrap_or(range);
    Some((name, "constant", range))
}

fn javascript_like_retrievable_exported_value(value: Node<'_>) -> bool {
    let value = unwrap_javascript_like_expression(value);
    if value.kind() == "new_expression" {
        return true;
    }
    if value.kind() == "call_expression"
        && value
            .child_by_field_name("function")
            .is_some_and(|function| function.kind() == "member_expression")
    {
        return true;
    }

    matches!(value.kind(), "object" | "array")
}

fn unwrap_javascript_like_expression(mut node: Node<'_>) -> Node<'_> {
    for _ in 0..4 {
        if matches!(
            node.kind(),
            "as_expression" | "satisfies_expression" | "parenthesized_expression"
        ) && let Some(inner) = node
            .child_by_field_name("value")
            .or_else(|| node.child_by_field_name("left"))
            .or_else(|| node.named_child(0))
        {
            node = inner;
            continue;
        }
        break;
    }

    node
}

fn export_statement_ancestor(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..4 {
        let parent = node.parent()?;
        if parent.kind() == "export_statement" {
            return Some(parent);
        }
        node = parent;
    }

    None
}

fn function_value_node(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "assignment_expression" => node.child_by_field_name("right"),
        "pair" | "public_field_definition" | "variable_declarator" => {
            node.child_by_field_name("value")
        }
        _ => None,
    }
}

fn function_value_name(content: &str, node: Node<'_>) -> Option<String> {
    match node.kind() {
        "assignment_expression" => {
            assignment_target_name(content, node.child_by_field_name("left")?)
        }
        "pair" => named_property_text(content, node.child_by_field_name("key")?),
        "public_field_definition" | "variable_declarator" => {
            named_property_text(content, node.child_by_field_name("name")?)
        }
        _ => None,
    }
}

fn assignment_target_name(content: &str, target: Node<'_>) -> Option<String> {
    match target.kind() {
        "identifier" => named_property_text(content, target),
        "member_expression" => {
            let property = target.child_by_field_name("property")?;
            let name = named_property_text(content, property)?;
            if name == "exports"
                && target
                    .child_by_field_name("object")
                    .is_some_and(|object| node_text(content, object) == "module")
            {
                return None;
            }
            Some(name)
        }
        _ => None,
    }
}

fn named_property_text(content: &str, node: Node<'_>) -> Option<String> {
    matches!(
        node.kind(),
        "identifier" | "private_property_identifier" | "property_identifier"
    )
    .then(|| node_text(content, node))
}

fn javascript_identifier_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|character| {
        character == '_' || character == '$' || character.is_ascii_alphabetic()
    }) && characters
        .all(|character| character == '_' || character == '$' || character.is_ascii_alphanumeric())
}
