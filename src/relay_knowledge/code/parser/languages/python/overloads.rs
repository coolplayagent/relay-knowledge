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
    let mut crossed_scope = false;
    let mut delayed_lookup = false;
    'lookup: loop {
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        if delayed_lookup
            && node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "module" | "block")
                    && (!crossed_scope || !class_namespace(parent))
            })
            && later_binding(content, node, binding, module, &mut remaining)
        {
            return false;
        }
        // Only module/block children are lexical statements. Parameter and
        // annotation siblings of a body are not preceding assignments.
        let mut previous = node
            .parent()
            .filter(|parent| matches!(parent.kind(), "module" | "block"))
            .filter(|parent| !crossed_scope || !class_namespace(*parent))
            .and_then(|_| node.prev_named_sibling());
        while let Some(statement) = previous {
            if remaining == 0 {
                return false;
            }
            remaining -= 1;
            if matches!(statement.kind(), "global_statement" | "nonlocal_statement")
                && contains_identifier(content, statement, binding)
            {
                let global = statement.kind() == "global_statement";
                if global
                    && statement
                        .parent()
                        .is_some_and(|parent| parent.kind() == "module")
                {
                    previous = statement.prev_named_sibling();
                    continue;
                }
                // Directives redirect lookup; unlike assignment/deletion they
                // do not establish a new value for the decorator binding.
                while let Some(parent) = node.parent() {
                    if remaining == 0 {
                        return false;
                    }
                    remaining -= 1;
                    if global && parent.kind() == "module" {
                        break;
                    }
                    node = parent;
                    if !global && matches!(node.kind(), "function_definition" | "class_definition")
                    {
                        break;
                    }
                }
                crossed_scope = true;
                delayed_lookup = true;
                continue 'lookup;
            }
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
        // A method's decorators can use their immediate class namespace, but
        // nested functions/classes do not close over an enclosing class body.
        delayed_lookup |= parent.kind() == "function_definition";
        crossed_scope |= matches!(parent.kind(), "function_definition" | "class_definition");
        node = parent;
    }
}

fn later_binding(
    content: &str,
    node: Node<'_>,
    binding: &str,
    module: bool,
    remaining: &mut usize,
) -> bool {
    let mut next = node.next_named_sibling();
    while let Some(statement) = next {
        if *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        // Outer names are read when the nested callable executes. A later
        // write prevents proving that an earlier typing import is still bound.
        if !matches!(statement.kind(), "global_statement" | "nonlocal_statement")
            && statement_binding(content, statement, binding, module).is_some()
        {
            return true;
        }
        next = statement.next_named_sibling();
    }
    false
}

fn class_namespace(mut node: Node<'_>) -> bool {
    for _ in 0..MAX_BINDING_STATEMENTS {
        match node.kind() {
            "class_definition" => return true,
            "function_definition" | "module" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
    // An unproven namespace must not supply a typing import binding.
    true
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
    if statement.kind() == "try_statement" {
        return try_import_binding(content, statement, binding, module);
    }
    simple_binding(content, statement, binding, module)
}

fn simple_binding(content: &str, statement: Node<'_>, binding: &str, module: bool) -> Option<bool> {
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
            .filter(|left| assignment_binds(content, *left, binding, module))
            .map(|_| false);
    }
    // Control-flow and deletion can change a binding. Do not guess its value.
    contains_identifier(content, statement, binding).then_some(false)
}

fn try_import_binding(content: &str, node: Node<'_>, binding: &str, module: bool) -> Option<bool> {
    if !contains_identifier(content, node, binding) {
        return None;
    }
    let mut remaining = MAX_BINDING_STATEMENTS;
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
            return Some(false);
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
            if let Some(value) = simple_binding(content, statement, binding, module) {
                last_binding = Some(value);
            }
        }
        if matches!(branch.kind(), "block" | "except_clause") {
            if last_binding != Some(true) {
                return Some(false);
            }
        } else if last_binding == Some(false) {
            return Some(false);
        }
    }
    Some(true)
}

fn assignment_binds(content: &str, node: Node<'_>, name: &str, module: bool) -> bool {
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
        if module
            && matches!(current.kind(), "attribute" | "subscript")
            && module_receiver(content, current, name)
        {
            return true;
        }
        if !matches!(current.kind(), "attribute" | "subscript") && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
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

fn module_receiver(content: &str, mut node: Node<'_>, name: &str) -> bool {
    let mut access = None;
    for _ in 0..MAX_BINDING_STATEMENTS {
        let Some(receiver) = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"))
        else {
            return false;
        };
        if receiver.kind() == "identifier" {
            if node_text(content, receiver) != name {
                return false;
            }
            if node.kind() != "attribute" {
                return true;
            }
            let member = node
                .child_by_field_name("attribute")
                .map(|n| node_text(content, n));
            return match member.as_deref() {
                Some("overload") => true,
                Some("__dict__") => namespace_may_write_overload(content, access),
                _ => false,
            };
        }
        access = Some(node);
        node = receiver;
    }
    true
}

fn namespace_may_write_overload(content: &str, access: Option<Node<'_>>) -> bool {
    let Some(key) = access
        .filter(|n| n.kind() == "subscript")
        .and_then(|n| n.child_by_field_name("subscript"))
    else {
        return true;
    };
    let literal = node_text(content, key);
    // Unknown/escaped expressions may name overload. Only a plain static key
    // can prove that a different namespace member is being written.
    if key.kind() != "string" || literal.contains('\\') {
        return true;
    }
    let Some(quote) = literal.chars().next().filter(|q| matches!(q, '\'' | '"')) else {
        return true;
    };
    if !literal.ends_with(quote)
        || literal.len() < 2
        || literal.starts_with(&quote.to_string().repeat(3))
    {
        return true;
    }
    &literal[1..literal.len() - 1] == "overload"
}
