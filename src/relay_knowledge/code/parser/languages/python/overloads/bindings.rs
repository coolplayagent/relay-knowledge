//! Bounded statement binding proofs and completing-branch merges.
use crate::code::parser::nodes::node_text;
use crate::code::python_imports::PythonModuleOrigins;
use tree_sitter::Node;
const MAX_BINDING_STATEMENTS: usize = 1024;

pub(super) fn statement_binding(
    content: &str,
    statement: Node<'_>,
    binding: &str,
    module: bool,
    origins: PythonModuleOrigins,
) -> Option<bool> {
    if statement.kind() == "try_statement" {
        return try_import_binding(content, statement, binding, module, origins);
    }
    simple_binding(content, statement, binding, module, origins)
}

fn simple_binding(
    content: &str,
    statement: Node<'_>,
    binding: &str,
    module: bool,
    origins: PythonModuleOrigins,
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
        let mut imported_binding = None;
        for import in statement.children_by_field_name("name", &mut cursor) {
            let name = import.child_by_field_name("name").unwrap_or(import);
            let imported = node_text(content, name);
            let local = import
                .child_by_field_name("alias")
                .map(|alias| node_text(content, alias))
                .unwrap_or_else(|| imported.split('.').next().unwrap_or_default().to_owned());
            if local == binding {
                imported_binding = Some(if module {
                    from.is_none() && origins.permits_standard_module(&imported)
                } else {
                    imported == "overload"
                        && from
                            .as_deref()
                            .is_some_and(|name| origins.permits_standard_module(name))
                });
            }
        }
        return imported_binding;
    }
    let definition = if statement.kind() == "decorated_definition" {
        statement
            .child_by_field_name("definition")
            .unwrap_or(statement)
    } else {
        statement
    };
    if matches!(
        definition.kind(),
        "function_definition" | "class_definition"
    ) && definition
        .child_by_field_name("name")
        .is_some_and(|name| node_text(content, name) == binding)
    {
        return Some(false);
    }
    if matches!(
        statement.kind(),
        "if_statement" | "for_statement" | "while_statement" | "with_statement" | "match_statement"
    ) && nested_declaration(content, statement, binding)
    {
        return Some(false);
    }
    if module && super::mutations::expression_mutates_module(content, statement, binding) {
        return Some(false);
    }
    super::mutations::expression_rebinds(content, statement, binding, module).then_some(false)
}

fn try_import_binding(
    content: &str,
    node: Node<'_>,
    binding: &str,
    module: bool,
    origins: PythonModuleOrigins,
) -> Option<bool> {
    if !contains_identifier(content, node, binding) {
        return None;
    }
    let mut remaining = MAX_BINDING_STATEMENTS;
    let mut merged = true;
    let mut cursor = node.walk();
    for branch in node.named_children(&mut cursor) {
        if branch
            .child_by_field_name("alias")
            .or_else(|| {
                branch
                    .child_by_field_name("value")
                    .filter(|value| value.kind() == "as_pattern")
                    .and_then(|value| value.child_by_field_name("alias"))
            })
            .is_some_and(|alias| contains_identifier(content, alias, binding))
        {
            merged = false;
        }
        let body = if branch.kind() == "block" {
            Some(branch)
        } else {
            let mut cursor = branch.walk();
            branch
                .named_children(&mut cursor)
                .find(|child| child.kind() == "block")
        };
        let Some(body) = body else {
            return Some(false);
        };
        let mut last_binding = None;
        let mut cursor = body.walk();
        for statement in body.named_children(&mut cursor) {
            if remaining == 0 {
                return Some(false);
            }
            remaining -= 1;
            // Nested control flow is intentionally unknown; this merge accepts
            // only independently proven imports on every completing path.
            if let Some(value) = simple_binding(content, statement, binding, module, origins) {
                last_binding = Some(value);
            } else if super::expressions::has_eager_call(content, statement, &mut remaining) {
                last_binding = Some(false);
            }
        }
        if branch.kind() == "finally_clause" {
            if let Some(value) = last_binding {
                merged = value;
            }
        } else if matches!(branch.kind(), "block" | "except_clause") {
            merged &= last_binding == Some(true);
        } else if last_binding == Some(false) {
            merged = false;
        }
    }
    Some(merged)
}

pub(super) fn contains_identifier(content: &str, node: Node<'_>, name: &str) -> bool {
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

/// Python determines a function's local names from its whole lexical body,
/// even when the assigning statement has not executed at the decorator yet.
pub(super) fn function_local(
    content: &str,
    body: Node<'_>,
    binding: &str,
    remaining: &mut usize,
) -> bool {
    let mut stack = vec![body];
    let mut bound = false;
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if matches!(node.kind(), "global_statement" | "nonlocal_statement")
            && contains_identifier(content, node, binding)
        {
            return false;
        }
        if node.kind() != "block"
            && simple_binding(
                content,
                node,
                binding,
                false,
                PythonModuleOrigins::default(),
            )
            .is_some()
        {
            bound = true;
        }
        if matches!(
            node.kind(),
            "function_definition" | "class_definition" | "decorated_definition" | "lambda"
        ) {
            continue;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            *remaining = left;
            stack.push(child);
        }
    }
    bound
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;

/// A conditional import/definition is a real possible binding, unlike a read
/// of the same identifier in a condition or an attribute assignment target.
fn nested_declaration(content: &str, node: Node<'_>, binding: &str) -> bool {
    let mut stack = vec![node];
    let mut remaining = MAX_BINDING_STATEMENTS;
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        remaining = left;
        if matches!(current.kind(), "import_statement" | "import_from_statement") {
            if simple_binding(
                content,
                current,
                binding,
                false,
                PythonModuleOrigins::default(),
            )
            .is_some()
            {
                return true;
            }
            continue;
        }
        if matches!(current.kind(), "function_definition" | "class_definition") {
            if current
                .child_by_field_name("name")
                .is_some_and(|name| node_text(content, name) == binding)
            {
                return true;
            }
            continue;
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            remaining = left;
            stack.push(child);
        }
    }
    false
}
