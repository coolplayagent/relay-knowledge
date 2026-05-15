use tree_sitter::Node;

use super::{
    SyntaxRange, first_named_child_of_kind, node_text, push_children_reverse, syntax_range,
};

pub(super) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "function_definition" => node
            .child_by_field_name("declarator")
            .and_then(|declarator| declarator_name(content, declarator))
            .map(|name| vec![(name, "function", syntax_range(node))])
            .unwrap_or_default(),
        "declaration" if !has_ancestor_kind(node, "compound_statement") => {
            if is_typedef_declaration(content, node) {
                Vec::new()
            } else {
                function_declaration_symbols(content, node)
            }
        }
        "preproc_def" | "preproc_function_def" => node
            .child_by_field_name("name")
            .or_else(|| first_named_child_of_kind(node, "identifier"))
            .map(|name| vec![(node_text(content, name), "macro", syntax_range(node))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn function_declaration_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut cursor = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            let function_declarator = direct_function_declarator(declarator)?;
            let name = declarator_name(content, function_declarator)?;

            Some((name, "function_declaration", syntax_range(declaration)))
        })
        .collect()
}

fn is_typedef_declaration(content: &str, declaration: Node<'_>) -> bool {
    let mut stack = vec![declaration];
    while let Some(node) = stack.pop() {
        if node.kind() == "storage_class_specifier" && node_text(content, node) == "typedef" {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

fn direct_function_declarator(declarator: Node<'_>) -> Option<Node<'_>> {
    let mut stack = vec![declarator];
    while let Some(node) = stack.pop() {
        if node.kind() == "parameter_declaration" {
            continue;
        }
        if node.kind() == "function_declarator" && !is_function_pointer_variable(node) {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}

fn is_function_pointer_variable(function_declarator: Node<'_>) -> bool {
    function_declarator
        .child_by_field_name("declarator")
        .is_some_and(has_parenthesized_pointer_declarator)
}

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(current.kind(), "identifier" | "field_identifier") {
            return Some(node_text(content, current));
        }
        if let Some(declarator) = current.child_by_field_name("declarator") {
            stack.push(declarator);
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    None
}

fn contains_node_kind(root: Node<'_>, kind: &str) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return true;
        }
        push_children_reverse(node, &mut stack);
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

fn has_parenthesized_pointer_declarator(root: Node<'_>) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "parenthesized_declarator"
            && contains_node_kind(node, "pointer_declarator")
        {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}
