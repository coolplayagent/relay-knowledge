use tree_sitter::Node;

use super::{
    super::CodeIndexError,
    FileParseContext, FileParseOutput, language_nodes,
    nodes::{SyntaxRange, last_identifier_text, node_text, push_children_reverse, syntax_range},
    records::{reference_record, symbol_record, upsert_reference, upsert_symbol},
};

const MAX_EXPORTED_VALUE_LINES: usize = 64;

pub(super) fn collect_manual_nodes(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if manual_definition_candidate(context.language_id, node.kind()) {
            for (name, kind, range) in
                manual_definitions(context.content, context.language_id, node)
            {
                upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
            }
        }
        if let Some((name, range)) = manual_call(context, node) {
            upsert_reference(output, reference_record(context, &name, "call", &range)?);
        }
        if let Some((name, kind, range)) = manual_reference(context, node) {
            upsert_reference(output, reference_record(context, &name, kind, &range)?);
        }
        push_children_reverse(node, &mut stack);
    }

    Ok(())
}

fn manual_definition_candidate(language_id: &str, node_kind: &str) -> bool {
    match language_id {
        "c" => {
            matches!(
                node_kind,
                "declaration" | "preproc_def" | "preproc_function_def" | "call_expression"
            ) || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        "cpp" => {
            node_kind == "function_definition"
                || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        "javascript" | "jsx" | "typescript" | "tsx" => {
            matches!(
                node_kind,
                "assignment_expression"
                    | "pair"
                    | "public_field_definition"
                    | "variable_declarator"
            ) || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        _ => language_nodes::definition_kind(language_id, node_kind).is_some(),
    }
}

pub(super) fn manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if language_id == "c" {
        let definitions = super::parser_c::manual_definitions(content, node);
        if !definitions.is_empty() {
            return definitions;
        }
    }
    if language_id == "cpp" {
        let definitions = super::parser_cpp::manual_definitions(content, node);
        if !definitions.is_empty() {
            return definitions;
        }
    }
    if let Some(definition) = javascript_like_function_value_definition(content, language_id, node)
    {
        return vec![definition];
    }
    if let Some(definition) = javascript_like_exported_value_definition(content, language_id, node)
    {
        return vec![definition];
    }

    generic_manual_definition(content, language_id, node)
        .into_iter()
        .collect()
}

fn javascript_like_function_value_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || !matches!(
            node.kind(),
            "assignment_expression" | "pair" | "public_field_definition" | "variable_declarator"
        )
    {
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

    Some((name, "function", syntax_range(node)))
}

fn javascript_like_function_value(owner: Node<'_>, value: Node<'_>) -> bool {
    if javascript_like_function_node(value) {
        return true;
    }
    matches!(owner.kind(), "pair" | "public_field_definition")
        && javascript_like_function_factory_call(value, 0)
}

const MAX_FUNCTION_FACTORY_CALL_DEPTH: usize = 4;

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

fn javascript_like_exported_value_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || node.kind() != "variable_declarator"
        || !has_export_statement_ancestor(node)
    {
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
        ) {
            if let Some(inner) = node
                .child_by_field_name("value")
                .or_else(|| node.child_by_field_name("left"))
                .or_else(|| node.named_child(0))
            {
                node = inner;
                continue;
            }
        }
        break;
    }

    node
}

fn has_export_statement_ancestor(mut node: Node<'_>) -> bool {
    for _ in 0..4 {
        let Some(parent) = node.parent() else {
            return false;
        };
        if parent.kind() == "export_statement" {
            return true;
        }
        node = parent;
    }

    false
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

fn generic_manual_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let kind = language_nodes::definition_kind(language_id, node.kind())?;
    let name = node.child_by_field_name("name")?;

    Some((node_text(content, name), kind, syntax_range(node)))
}

fn manual_call(context: &FileParseContext<'_>, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    if node.kind() == "preproc_call" {
        let name = node
            .child_by_field_name("directive")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| super::nodes::first_named_child_of_kind(node, "identifier"))?;
        return Some((node_text(context.content, name), syntax_range(name)));
    }
    if !language_nodes::is_call_node(context.language_id, node.kind()) {
        return None;
    }
    if let Some(call) = constructed_type_call(context.content, node) {
        return Some(call);
    }
    let function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))
        .or_else(|| node.child(0))?;

    callable_expression_name(context.content, function)
}

fn callable_expression_name(content: &str, function: Node<'_>) -> Option<(String, SyntaxRange)> {
    if function.kind() == "subscript_expression" {
        let argument = function.child_by_field_name("argument");
        for index in 0..function.named_child_count() {
            let child = function.named_child(u32::try_from(index).ok()?)?;
            if argument.is_some_and(|argument| node_contains(argument, child)) {
                continue;
            }
            if let Some(callable) = callable_expression_name(content, child) {
                return Some(callable);
            }
        }
    }

    last_identifier_text(content, function).map(|name| (name, syntax_range(function)))
}

fn manual_reference(
    context: &FileParseContext<'_>,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(context.language_id, "c" | "cpp") || !c_family_reference_node(node.kind()) {
        return None;
    }
    let name = node_text(context.content, node);
    if !c_family_reference_name(&name) {
        return None;
    }
    if node.kind() == "type_identifier" && c_family_type_reference_context(node) {
        return Some((name, "type", syntax_range(node)));
    }
    if c_family_value_reference_context(node) {
        return Some((name, "implementation", syntax_range(node)));
    }

    None
}

fn c_family_reference_node(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "field_identifier" | "namespace_identifier" | "type_identifier"
    )
}

fn c_family_reference_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn c_family_type_reference_context(node: Node<'_>) -> bool {
    has_ancestor_kind(node, "field_declaration")
        || has_ancestor_kind(node, "parameter_declaration")
        || has_ancestor_kind(node, "qualified_type_identifier")
        || has_ancestor_kind(node, "scoped_type_identifier")
}

fn c_family_value_reference_context(node: Node<'_>) -> bool {
    has_ancestor_kind(node, "initializer_list")
        || has_non_argument_subscript_ancestor(node)
        || has_ancestor_kind(node, "qualified_identifier")
        || has_ancestor_kind(node, "scoped_identifier")
}

fn has_non_argument_subscript_ancestor(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "subscript_expression" {
            return !parent
                .child_by_field_name("argument")
                .is_some_and(|argument| node_contains(argument, node));
        }
        node = parent;
    }

    false
}

fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }

    false
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}

fn constructed_type_call(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    let type_node = node.child_by_field_name("type")?;
    let name = constructed_type_name(content, type_node)?;

    Some((name, syntax_range(type_node)))
}

fn constructed_type_name(content: &str, node: Node<'_>) -> Option<String> {
    match node.kind() {
        "generic_type" => first_constructed_type_child(node)
            .and_then(|inner| constructed_type_name(content, inner)),
        "array_type" => node
            .child_by_field_name("element")
            .and_then(|inner| constructed_type_name(content, inner)),
        "scoped_type_identifier" => last_type_identifier_text(content, node),
        "identifier" | "type_identifier" => Some(node_text(content, node)),
        _ => last_type_identifier_text(content, node),
    }
}

fn first_constructed_type_child(node: Node<'_>) -> Option<Node<'_>> {
    (0..node.child_count()).find_map(|index| {
        let index = u32::try_from(index).ok()?;
        let child = node.child(index)?;
        constructed_type_node(child.kind()).then_some(child)
    })
}

fn last_type_identifier_text(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = Vec::with_capacity(node.child_count().saturating_add(1));
    stack.push(node);
    let mut last = None;
    while let Some(current) = stack.pop() {
        if current.kind() == "type_arguments" {
            continue;
        }
        if matches!(current.kind(), "identifier" | "type_identifier") {
            last = Some(node_text(content, current));
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    last
}

fn constructed_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_type" | "generic_type" | "identifier" | "scoped_type_identifier" | "type_identifier"
    )
}
