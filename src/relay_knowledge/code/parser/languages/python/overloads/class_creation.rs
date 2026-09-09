//! Prove class construction cannot invoke unknown metaclass or descriptor code.
use tree_sitter::Node;

pub(super) fn plain(
    content: &str,
    node: Node<'_>,
    remaining: &mut usize,
    origins: crate::code::python_imports::PythonModuleOrigins,
) -> bool {
    if node.child_by_field_name("superclasses").is_some() {
        return false;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut stack = vec![body];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        match current.kind() {
            "function_definition"
            | "pass_statement"
            | "comment"
            | "global_statement"
            | "nonlocal_statement" => continue,
            "import_statement" => {
                if !known_module_import(content, current, remaining, origins) {
                    return false;
                }
                continue;
            }
            "import_from_statement" => {
                if !known_function_import(content, current, remaining, origins) {
                    return false;
                }
                continue;
            }
            "decorated_definition" => {
                // Decorator invocation is independently checked against the
                // node-specific proof cache by the eager-effect walker.
                if current
                    .child_by_field_name("definition")
                    .is_none_or(|n| n.kind() != "function_definition")
                {
                    return false;
                }
                continue;
            }
            "assignment" => {
                if let Some(value) = current.child_by_field_name("right") {
                    if !literal(value, remaining) && !function_value(content, value, remaining) {
                        return false;
                    }
                }
                continue;
            }
            "string" => continue,
            "block" | "expression_statement" => {}
            _ => return false,
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            stack.push(child);
        }
    }
    true
}

fn known_module_import(
    content: &str,
    node: Node<'_>,
    remaining: &mut usize,
    origins: crate::code::python_imports::PythonModuleOrigins,
) -> bool {
    let mut cursor = node.walk();
    let mut found = false;
    for imported in node.children_by_field_name("name", &mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        found = true;
        let name = imported.child_by_field_name("name").unwrap_or(imported);
        if !origins.permits_standard_module(&crate::code::parser::nodes::node_text(content, name)) {
            // Import executes module top-level code before the next decorator.
            // Arbitrary modules can mutate the containing namespace through
            // sys.modules even when the local import target is unrelated.
            return false;
        }
    }
    found
}

fn known_function_import(
    content: &str,
    node: Node<'_>,
    remaining: &mut usize,
    origins: crate::code::python_imports::PythonModuleOrigins,
) -> bool {
    use crate::code::parser::nodes::node_text;
    if !node
        .child_by_field_name("module_name")
        .is_some_and(|name| origins.permits_standard_module(&node_text(content, name)))
    {
        return false;
    }
    let mut cursor = node.walk();
    let mut found = false;
    for imported in node.children_by_field_name("name", &mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        found = true;
        let name = imported.child_by_field_name("name").unwrap_or(imported);
        if node_text(content, name) != "overload" {
            return false;
        }
    }
    found
}

fn function_value(content: &str, node: Node<'_>, remaining: &mut usize) -> bool {
    let Some(value) =
        super::expressions::transparent(node, remaining).filter(|n| n.kind() == "identifier")
    else {
        return false;
    };
    let name = crate::code::parser::nodes::node_text(content, value);
    let mut current = value;
    while let Some(parent) = current.parent() {
        *remaining = match remaining.checked_sub(1) {
            Some(left) => left,
            None => return false,
        };
        if matches!(parent.kind(), "module" | "block") {
            let mut previous = current.prev_named_sibling();
            while let Some(statement) = previous {
                *remaining = match remaining.checked_sub(1) {
                    Some(left) => left,
                    None => return false,
                };
                if statement.kind() == "function_definition" {
                    if statement
                        .child_by_field_name("name")
                        .is_some_and(|n| crate::code::parser::nodes::node_text(content, n) == name)
                    {
                        return true;
                    }
                } else if !matches!(statement.kind(), "comment" | "pass_statement") {
                    // A write, import, or unknown execution can replace the
                    // value; do not infer descriptor safety through it.
                    return false;
                }
                previous = statement.prev_named_sibling();
            }
        }
        current = parent;
    }
    false
}

fn literal(node: Node<'_>, remaining: &mut usize) -> bool {
    let Some(node) = super::expressions::transparent(node, remaining) else {
        return false;
    };
    matches!(
        node.kind(),
        "integer" | "float" | "string" | "true" | "false" | "none"
    )
}

#[cfg(test)]
#[path = "class_creation_tests.rs"]
mod tests;
