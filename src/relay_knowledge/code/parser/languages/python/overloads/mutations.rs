//! Bounded mutation targets affecting a proven typing module's overload value.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;
const MAX_BINDING_STATEMENTS: usize = 1024;
pub(super) fn expression_rebinds(content: &str, node: Node<'_>, name: &str, module: bool) -> bool {
    let mut remaining = MAX_BINDING_STATEMENTS;
    let deferred = super::expressions::future_annotations(content, node, &mut remaining);
    let origin_scope = lexical_scope(node, &mut remaining);
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        let target = match current.kind() {
            "assignment"
                if current.child_by_field_name("right").is_some()
                    || super::expressions::annotation_binds_local(current, &mut remaining) =>
            {
                current.child_by_field_name("left")
            }
            "for_statement" => current.child_by_field_name("left"),
            "as_pattern" => current.child_by_field_name("alias"),
            "delete_statement" => current.named_child(0),
            "augmented_assignment" => current.child_by_field_name("left"),
            "named_expression" => current.child_by_field_name("name"),
            _ => None,
        };
        let class_scope = lexical_scope(current, &mut remaining)
            .filter(|scope| scope.kind() == "class_definition" && Some(*scope) != origin_scope);
        let foreign_class =
            class_scope.is_some_and(|scope| !class_global(content, scope, name, &mut remaining));
        if target.is_some_and(|target| {
            if foreign_class {
                return module && member_target(content, target, name, &mut remaining);
            }
            assignment_binds(content, target, name, module, &mut remaining)
        }) {
            return true;
        }
        if current.kind() == "class_definition"
            && (module || class_global(content, current, name, &mut remaining))
        {
            if let Some(body) = current.child_by_field_name("body") {
                stack.push(body);
            }
        }
        if !super::expressions::eager_children(current, &mut stack, &mut remaining, deferred) {
            return true;
        }
    }
    false
}

fn assignment_binds(
    content: &str,
    node: Node<'_>,
    name: &str,
    module: bool,
    remaining: &mut usize,
) -> bool {
    let mut cursor = node.walk();
    loop {
        if *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        let current = cursor.node();
        if current.kind() == "identifier" && node_text(content, current) == name {
            return true;
        }
        if module
            && matches!(current.kind(), "attribute" | "subscript")
            && module_receiver(content, current, name, remaining)
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

fn module_receiver(content: &str, mut node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let mut access = None;
    while *remaining > 0 {
        *remaining -= 1;
        let Some(receiver) = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"))
            .and_then(|receiver| super::expressions::transparent(receiver, remaining))
        else {
            return *remaining == 0;
        };
        if receiver.kind() == "identifier" {
            if !super::aliases::refers_to(content, receiver, name, remaining) {
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
    key_may_name_overload(content, key)
}

fn key_may_name_overload(content: &str, key: Node<'_>) -> bool {
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

pub(super) fn expression_mutates_module(
    content: &str,
    expression: Node<'_>,
    binding: &str,
) -> bool {
    let mut remaining = MAX_BINDING_STATEMENTS;
    let deferred = super::expressions::future_annotations(content, expression, &mut remaining);
    let mut stack = vec![expression];
    while let Some(node) = stack.pop() {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        if node.kind() == "call"
            && mutator_member_effect(content, node, binding, &mut remaining) == Some(true)
        {
            return true;
        }
        if !super::expressions::eager_children(node, &mut stack, &mut remaining, deferred) {
            return true;
        }
    }
    false
}

fn mutator_member_effect(
    content: &str,
    node: Node<'_>,
    binding: &str,
    remaining: &mut usize,
) -> Option<bool> {
    let function = node
        .child_by_field_name("function")
        .filter(|function| function.kind() == "identifier")?;
    if !matches!(node_text(content, function).as_str(), "setattr" | "delattr") {
        return None;
    }
    if !builtin_unbound(content, node, &node_text(content, function), remaining) {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    let Some(receiver) = arguments
        .named_child(0)
        .and_then(|receiver| super::expressions::transparent(receiver, remaining))
        .filter(|receiver| receiver.kind() == "identifier")
    else {
        return (*remaining == 0).then_some(true);
    };
    let may_select = arguments
        .named_child(1)
        .is_none_or(|key| key_may_name_overload(content, key));
    if !may_select {
        return Some(false);
    }
    super::aliases::refers_to(content, receiver, binding, remaining).then_some(true)
}

#[cfg(test)]
#[path = "mutations_tests.rs"]
mod tests;

/// An explicit write to another typing member is not an unknown namespace call.
/// Nested argument calls still consume this same walk and invalidate the proof.
pub(super) fn unknown_eager_call(
    content: &str,
    statement: Node<'_>,
    binding: &str,
    module: bool,
    proven_decorators: &std::collections::BTreeMap<usize, bool>,
    remaining: &mut usize,
    origins: crate::code::python_imports::PythonModuleOrigins,
) -> bool {
    let deferred = super::expressions::future_annotations(content, statement, remaining);
    let mut stack = vec![statement];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if node.kind() == "decorator" && proven_decorators.get(&node.start_byte()) != Some(&true) {
            return true;
        }
        if node.kind() == "class_definition" {
            if !super::class_creation::plain(content, node, remaining, origins) {
                return true;
            }
            if let Some(body) = node.child_by_field_name("body") {
                stack.push(body);
            }
        }
        if node.kind() == "call"
            && (!module || mutator_member_effect(content, node, binding, remaining) != Some(false))
        {
            return true;
        }
        if !super::expressions::eager_children(node, &mut stack, remaining, deferred) {
            return true;
        }
    }
    false
}

fn class_global(content: &str, node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut stack = vec![body];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if current.kind() == "global_statement"
            && assignment_binds(content, current, name, false, remaining)
        {
            return true;
        }
        if matches!(
            current.kind(),
            "function_definition" | "class_definition" | "decorated_definition" | "lambda"
        ) {
            continue;
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            *remaining = left;
            stack.push(child);
        }
    }
    false
}

/// A builtin-name safety exception is unavailable when any explicit source
/// binding can replace it. Unknown or oversized source is never assumed pure.
fn builtin_unbound(content: &str, mut node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    while let Some(parent) = node.parent() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        node = parent;
    }
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        let target = match current.kind() {
            "assignment" | "augmented_assignment" | "for_statement" => {
                current.child_by_field_name("left")
            }
            "named_expression" | "function_definition" | "class_definition" => {
                current.child_by_field_name("name")
            }
            "as_pattern" => current.child_by_field_name("alias"),

            _ => None,
        };
        if target.is_some_and(|n| assignment_binds(content, n, name, false, remaining)) {
            return false;
        }
        if matches!(current.kind(), "parameters" | "lambda_parameters") {
            let mut cursor = current.walk();
            for parameter in current.named_children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return false;
                };
                *remaining = left;
                let target = match parameter.kind() {
                    "identifier" => Some(parameter),
                    "default_parameter" | "typed_default_parameter" => {
                        parameter.child_by_field_name("name")
                    }
                    "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                        parameter.named_child(0)
                    }
                    _ => None,
                };
                if target.is_some_and(|n| assignment_binds(content, n, name, false, remaining)) {
                    return false;
                }
            }
        }
        if matches!(current.kind(), "import_statement" | "import_from_statement") {
            let mut cursor = current.walk();
            for import in current.named_children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return false;
                };
                *remaining = left;
                if import.kind() == "wildcard_import" {
                    return false;
                }
                let local = import.child_by_field_name("alias").unwrap_or(import);
                if node_text(content, local) == name {
                    return false;
                }
            }
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

fn lexical_scope<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        *remaining = remaining.checked_sub(1)?;
        if parent.kind() == "module"
            || (matches!(
                parent.kind(),
                "class_definition" | "function_definition" | "lambda"
            ) && parent.child_by_field_name("body") == Some(node))
        {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn member_target(content: &str, node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if matches!(current.kind(), "attribute" | "subscript") {
            if module_receiver(content, current, name, remaining) {
                return true;
            }
            continue;
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            *remaining = left;
            stack.push(child);
        }
    }
    false
}
