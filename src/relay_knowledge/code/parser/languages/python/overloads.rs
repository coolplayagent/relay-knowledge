//! Recognize overload declarations only through a visible typing import binding.
use tree_sitter::Node;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};

const MAX_BINDING_STATEMENTS: usize = 1024;

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if node.kind() != "function_definition" || !is_overload_declaration(content, node) {
        return Vec::new();
    }
    node.child_by_field_name("name")
        .map(|name| {
            vec![(
                node_text(content, name),
                "function_declaration",
                syntax_range(node),
            )]
        })
        .unwrap_or_default()
}

pub(in crate::code::parser) fn is_overload_declaration(content: &str, function: Node<'_>) -> bool {
    let Some(decorated) = function
        .parent()
        .filter(|node| node.kind() == "decorated_definition")
    else {
        return false;
    };
    let mut cursor = decorated.walk();
    decorated
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "decorator")
        .any(|decorator| {
            let Some(expression) = decorator.named_child(0) else {
                return false;
            };
            let (binding, module) = match expression.kind() {
                "identifier" => (node_text(content, expression), false),
                "attribute"
                    if expression
                        .child_by_field_name("attribute")
                        .is_some_and(|name| node_text(content, name) == "overload") =>
                {
                    let Some(object) = expression
                        .child_by_field_name("object")
                        .filter(|node| node.kind() == "identifier")
                    else {
                        return false;
                    };
                    (node_text(content, object), true)
                }
                _ => return false,
            };
            visible_import(content, decorated, &binding, module)
        })
}

fn visible_import(content: &str, mut node: Node<'_>, binding: &str, module: bool) -> bool {
    let mut remaining = MAX_BINDING_STATEMENTS;
    loop {
        // Only module/block children are lexical statements. Parameter and
        // annotation siblings of a body are not preceding assignments.
        let mut previous = node
            .parent()
            .filter(|parent| matches!(parent.kind(), "module" | "block"))
            .and_then(|_| node.prev_named_sibling());
        while let Some(statement) = previous {
            if remaining == 0 {
                return false;
            }
            remaining -= 1;
            if let Some(imported) = statement_binding(content, statement, binding, module) {
                return imported;
            }
            previous = statement.prev_named_sibling();
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        if parent.kind() == "function_definition"
            && parent
                .child_by_field_name("parameters")
                .is_some_and(|parameters| parameter_binds(content, parameters, binding))
        {
            return false;
        }
        node = parent;
    }
}

fn parameter_binds(content: &str, parameters: Node<'_>, binding: &str) -> bool {
    let mut cursor = parameters.walk();
    parameters.named_children(&mut cursor).any(|parameter| {
        let name = match parameter.kind() {
            "identifier" => Some(parameter),
            "default_parameter" | "typed_default_parameter" => {
                parameter.child_by_field_name("name")
            }
            "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                parameter.named_child(0)
            }
            _ => None,
        };
        name.is_some_and(|name| contains_identifier(content, name, binding))
    })
}

fn statement_binding(
    content: &str,
    statement: Node<'_>,
    binding: &str,
    module: bool,
) -> Option<bool> {
    if matches!(
        statement.kind(),
        "import_statement" | "import_from_statement"
    ) {
        let mut wildcard_cursor = statement.walk();
        if statement
            .named_children(&mut wildcard_cursor)
            .any(|node| node.kind() == "wildcard_import")
        {
            return Some(false);
        }
        let from = statement
            .child_by_field_name("module_name")
            .map(|name| node_text(content, name));
        let mut cursor = statement.walk();
        for import in statement.children_by_field_name("name", &mut cursor) {
            let name = import.child_by_field_name("name").unwrap_or(import);
            let imported = node_text(content, name);
            let local = import
                .child_by_field_name("alias")
                .map(|alias| node_text(content, alias))
                .unwrap_or_else(|| imported.split('.').next().unwrap_or_default().to_owned());
            if local == binding {
                return Some(if module {
                    from.is_none() && matches!(imported.as_str(), "typing" | "typing_extensions")
                } else {
                    imported == "overload"
                        && from
                            .as_deref()
                            .is_some_and(|name| matches!(name, "typing" | "typing_extensions"))
                });
            }
        }
        return None;
    }
    let statement = if statement.kind() == "decorated_definition" {
        statement
            .child_by_field_name("definition")
            .unwrap_or(statement)
    } else {
        statement
    };
    if matches!(statement.kind(), "function_definition" | "class_definition") {
        return statement
            .child_by_field_name("name")
            .filter(|name| node_text(content, *name) == binding)
            .map(|_| false);
    }
    if statement.kind() == "expression_statement" {
        let expression = statement.named_child(0)?;
        return expression
            .child_by_field_name("left")
            .filter(|left| contains_identifier(content, *left, binding))
            .map(|_| false);
    }
    // Control-flow and deletion can change a binding. Do not guess its value.
    contains_identifier(content, statement, binding).then_some(false)
}

fn contains_identifier(content: &str, node: Node<'_>, name: &str) -> bool {
    let mut cursor = node.walk();
    let mut remaining = MAX_BINDING_STATEMENTS;
    loop {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        let current = cursor.node();
        if current.kind() == "identifier" && node_text(content, current) == name {
            return true;
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}
