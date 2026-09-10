//! Implicit execution boundaries for Python overload provider proofs.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;

/// Literal built-in values cannot redirect execution through user protocols.
/// Unknown receivers remain unsafe even when the syntax contains no call node.
pub(super) fn dispatches(
    content: &str,
    node: Node<'_>,
    provider: Option<(&str, bool, crate::code::python_imports::PythonModuleOrigins)>,
    remaining: &mut usize,
) -> bool {
    let operand = match node.kind() {
        "await" => return true,
        "yield" => {
            // Even a literal yielded value can suspend this proof while other
            // code replaces the provider. Only empty built-in delegation has
            // neither user iteration nor a suspension point.
            let mut cursor = node.walk();
            let mut delegated = false;
            for child in node.children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return true;
                };
                *remaining = left;
                delegated |= child.kind() == "from";
            }
            return !delegated
                || !node
                    .named_child(0)
                    .and_then(|value| super::transparent::transparent(value, remaining))
                    .is_some_and(|value| {
                        matches!(value.kind(), "tuple" | "list" | "dictionary")
                            && value.named_child_count() == 0
                    });
        }
        "if_statement" | "elif_clause" | "while_statement" => {
            return node
                .child_by_field_name("condition")
                .is_none_or(|value| !truth_is_builtin(content, value, provider, remaining));
        }
        "assert_statement" | "if_clause" => {
            return node
                .named_child(0)
                .is_none_or(|value| !truth_is_builtin(content, value, provider, remaining));
        }
        "for_statement" | "for_in_clause" => {
            return node
                .child_by_field_name("right")
                .is_none_or(|value| !container_is_builtin(value, remaining));
        }
        "generator_expression" => {
            let mut cursor = node.walk();
            let mut iterable = None;
            for child in node.named_children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return true;
                };
                *remaining = left;
                if child.kind() == "for_in_clause" {
                    iterable = child.child_by_field_name("right");
                    break;
                }
            }
            iterable
        }
        "conditional_expression" => node.named_child(1),
        "boolean_operator" => return !truth_is_builtin(content, node, provider, remaining),
        "subscript" => return !subscript_is_builtin(content, node, provider, remaining),
        "binary_operator" | "comparison_operator" | "unary_operator" | "interpolation" => {
            Some(node)
        }
        "list_splat" | "dictionary_splat" => node.named_child(0),
        "with_item" => {
            return !provider.is_some_and(|(_, _, origins)| {
                super::local_instances::context_manager(content, node, remaining, origins)
            });
        }
        "augmented_assignment" => return true,
        "dictionary" | "set" => {
            let mut cursor = node.walk();
            for item in node.named_children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return true;
                };
                *remaining = left;
                let key = if node.kind() == "dictionary" {
                    item.child_by_field_name("key")
                } else if item.kind() == "comment" {
                    None
                } else {
                    Some(item)
                };
                if key.is_some_and(|key| !literal_value(key, remaining)) {
                    return true;
                }
            }
            return false;
        }
        "attribute" => {
            if standard_module_attribute(content, node, provider, remaining) {
                return false;
            }
            return node
                .child_by_field_name("attribute")
                .is_none_or(|n| node_text(content, n) == "__class__")
                || !node
                    .child_by_field_name("object")
                    .filter(|n| n.kind() == "identifier")
                    .is_some_and(|receiver| {
                        super::proven_aliases::plain_instance(content, receiver, remaining)
                            || provider.is_some_and(|(_, _, origins)| {
                                super::local_instances::instance(
                                    content, receiver, remaining, origins,
                                )
                                .is_some()
                            })
                    });
        }
        "assignment"
            if node.child_by_field_name("left").is_some_and(|target| {
                matches!(
                    target.kind(),
                    "pattern_list" | "tuple_pattern" | "list_pattern"
                )
            }) =>
        {
            node.child_by_field_name("right")
        }
        _ => return false,
    };
    operand.is_none_or(|operand| !literal_value(operand, remaining))
}

fn standard_module_attribute(
    content: &str,
    node: Node<'_>,
    provider: Option<(&str, bool, crate::code::python_imports::PythonModuleOrigins)>,
    remaining: &mut usize,
) -> bool {
    let Some((binding, module, origins)) = provider else {
        return false;
    };
    let Some(receiver) = node
        .child_by_field_name("object")
        .and_then(|node| super::transparent::transparent(node, remaining))
        .filter(|node| node.kind() == "identifier")
    else {
        return false;
    };
    // Changing a module's class can install custom attribute dispatch even
    // though ordinary reads on its proven standard-module receiver are safe.
    if node
        .child_by_field_name("attribute")
        .is_some_and(|attribute| {
            super::protocol_contract::MODULE_DISPATCH_HOOKS
                .contains(&node_text(content, attribute).as_str())
        })
    {
        return false;
    }
    // The mutation alias walk deliberately returns "may alias" for unknown
    // control flow. Such a result cannot certify a harmless attribute read.
    (module && super::proven_aliases::refers_to(content, receiver, binding, remaining))
        || super::imported_modules::standard(content, receiver, remaining, origins)
}

// Iterating or truth-testing a constructed built-in container does not invoke
// protocols on its elements. Their construction is still checked by the eager walk.
fn container_is_builtin(node: Node<'_>, remaining: &mut usize) -> bool {
    super::transparent::transparent(node, remaining).is_some_and(|node| {
        matches!(
            node.kind(),
            "tuple" | "list" | "dictionary" | "set" | "string" | "concatenated_string"
        )
    })
}

fn truth_is_builtin(
    content: &str,
    node: Node<'_>,
    provider: Option<(&str, bool, crate::code::python_imports::PythonModuleOrigins)>,
    remaining: &mut usize,
) -> bool {
    let Some(node) = super::transparent::transparent(node, remaining) else {
        return false;
    };
    if container_is_builtin(node, remaining) {
        return true;
    }
    if node.kind() == "identifier" {
        return provider.is_some_and(|(binding, _, origins)| {
            super::proven_aliases::refers_to(content, node, binding, remaining)
                || super::proven_aliases::imported_function(content, node, remaining, origins)
        });
    }
    if node.kind() == "boolean_operator" {
        let mut cursor = node.walk();
        return node.named_children(&mut cursor).all(|child| {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            child.kind() == "comment" || truth_is_builtin(content, child, provider, remaining)
        });
    }
    literal_value(node, remaining)
}

fn subscript_is_builtin(
    content: &str,
    node: Node<'_>,
    provider: Option<(&str, bool, crate::code::python_imports::PythonModuleOrigins)>,
    remaining: &mut usize,
) -> bool {
    let Some(mut object) = node
        .child_by_field_name("value")
        .and_then(|n| super::transparent::transparent(n, remaining))
    else {
        return false;
    };
    let Some(key) = node.child_by_field_name("subscript") else {
        return false;
    };
    if !literal_value(key, remaining)
        && !provider.is_some_and(|(binding, _, _)| {
            super::proven_aliases::refers_to(content, key, binding, remaining)
        })
    {
        return false;
    }
    if object.kind() == "attribute"
        && object
            .child_by_field_name("attribute")
            .is_some_and(|n| node_text(content, n) == "__dict__")
    {
        return standard_module_attribute(content, object, provider, remaining);
    }
    if object.kind() == "identifier" {
        let Some(value) = super::proven_aliases::preceding_value(
            content,
            object,
            &node_text(content, object),
            remaining,
        ) else {
            return false;
        };
        object = value;
    }
    // Only literal contents prove that dictionary equality cannot reach a
    // previously inserted custom key. Unknown indexable objects stay unsafe.
    literal_value(object, remaining)
}

fn literal_value(node: Node<'_>, remaining: &mut usize) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        match current.kind() {
            "integer" | "float" | "true" | "false" | "none" | "string_start" | "string_content"
            | "string_end" | "escape_sequence" | "comment" => continue,
            "string"
            | "concatenated_string"
            | "parenthesized_expression"
            | "tuple"
            | "list"
            | "dictionary"
            | "set"
            | "pair"
            | "binary_operator"
            | "boolean_operator"
            | "comparison_operator"
            | "unary_operator"
            | "subscript"
            | "interpolation"
            | "format_specifier"
            | "type_conversion" => {}
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

#[cfg(test)]
#[path = "implicit_protocols_tests.rs"]
mod tests;
