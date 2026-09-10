//! Same-block local class and instance evidence; unknown effects end the proof.
use std::collections::BTreeMap;
use tree_sitter::Node;

use super::local_classes::Class;
use crate::code::python_imports::PythonModuleOrigins;

#[derive(Clone, Copy)]
enum Binding {
    Class(Class),
    Instance(Class),
}

/// Every visited statement and retained binding consumes the caller's shared
/// budget. No proof crosses a lexical boundary or re-enters mutation analysis.
pub(super) fn instance(
    content: &str,
    receiver: Node<'_>,
    remaining: &mut usize,
    origins: PythonModuleOrigins,
) -> Option<Class> {
    let receiver = super::transparent::transparent(receiver, remaining)?;
    if receiver.kind() != "identifier" {
        return None;
    }
    let name = content.get(receiver.byte_range())?;
    let mut position = receiver;
    let block = loop {
        *remaining = remaining.checked_sub(1)?;
        let parent = position.parent()?;
        if matches!(parent.kind(), "module" | "block") {
            break parent;
        }
        position = parent;
    };
    let mut bindings = BTreeMap::new();
    let mut cursor = block.walk();
    for statement in block.named_children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        if statement == position {
            return match bindings.get(name) {
                Some(Binding::Instance(class)) => Some(*class),
                _ => None,
            };
        }
        record(content, statement, &mut bindings, remaining, origins)?;
    }
    None
}

fn record<'a>(
    content: &'a str,
    statement: Node<'_>,
    bindings: &mut BTreeMap<&'a str, Binding>,
    remaining: &mut usize,
    origins: PythonModuleOrigins,
) -> Option<()> {
    match statement.kind() {
        "comment" | "pass_statement" => {}
        "class_definition" => {
            let class = super::local_classes::classify(content, statement, remaining)?;
            let name = content.get(statement.child_by_field_name("name")?.byte_range())?;
            bindings.insert(name, Binding::Class(class));
        }
        "function_definition" => {
            if statement.child_by_field_name("return_type").is_some()
                || statement.child_by_field_name("type_parameters").is_some()
            {
                return None;
            }
            let parameters = statement.child_by_field_name("parameters")?;
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                if parameter.kind() != "identifier" {
                    return None;
                }
            }
            bindings.remove(content.get(statement.child_by_field_name("name")?.byte_range())?);
        }
        "import_statement" | "import_from_statement" => {
            let module = statement.child_by_field_name("module_name");
            if module.is_some_and(|n| {
                !content
                    .get(n.byte_range())
                    .is_some_and(|s| origins.permits_standard_module(s))
            }) {
                return None;
            }
            let mut cursor = statement.walk();
            for child in statement.named_children(&mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                if child.kind() == "wildcard_import" {
                    return None;
                }
            }
            let mut cursor = statement.walk();
            for imported in statement.children_by_field_name("name", &mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                let original = imported.child_by_field_name("name").unwrap_or(imported);
                if module.is_none()
                    && !origins.permits_standard_module(content.get(original.byte_range())?)
                {
                    return None;
                }
                let local = imported.child_by_field_name("alias").unwrap_or(original);
                bindings.remove(content.get(local.byte_range())?);
            }
        }
        "expression_statement" => {
            let assignment = statement.named_child(0)?;
            if assignment.kind() != "assignment" || assignment.child_by_field_name("type").is_some()
            {
                return None;
            }
            let target = assignment.child_by_field_name("left")?;
            if target.kind() != "identifier" {
                return None;
            }
            let value = super::transparent::transparent(
                assignment.child_by_field_name("right")?,
                remaining,
            )?;
            let binding = match value.kind() {
                "identifier" => bindings.get(content.get(value.byte_range())?).copied(),
                "call" => {
                    let constructor = value.child_by_field_name("function")?;
                    let arguments = value.child_by_field_name("arguments")?;
                    if constructor.kind() != "identifier" || arguments.named_child_count() != 0 {
                        return None;
                    }
                    let Binding::Class(class) =
                        *bindings.get(content.get(constructor.byte_range())?)?
                    else {
                        return None;
                    };
                    Some(Binding::Instance(class))
                }
                "integer" | "float" | "true" | "false" | "none" => None,
                _ => return None,
            };
            let name = content.get(target.byte_range())?;
            bindings.remove(name);
            if let Some(binding) = binding {
                bindings.insert(name, binding);
            }
        }
        _ => return None,
    }
    Some(())
}

pub(super) fn context_manager(
    content: &str,
    item: Node<'_>,
    remaining: &mut usize,
    origins: PythonModuleOrigins,
) -> bool {
    let Some(clause) = item.parent() else {
        return false;
    };
    // Earlier managers or async dispatch require independent execution proofs.
    if clause.named_child_count() != 1
        || clause
            .parent()
            .and_then(|n| n.child(0))
            .is_some_and(|n| n.kind() == "async")
    {
        return false;
    }
    let Some(mut value) = item.child_by_field_name("value") else {
        return false;
    };
    if value.kind() == "as_pattern" {
        let Some(expression) = value.named_child(0) else {
            return false;
        };
        value = expression;
    }
    instance(content, value, remaining, origins) == Some(Class::LiteralManager)
}

#[cfg(test)]
#[path = "local_instances_tests.rs"]
mod tests;
