//! Positive alias evidence is separate from conservative may-write analysis.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;

pub(super) fn refers_to(
    content: &str,
    mut value: Node<'_>,
    binding: &str,
    remaining: &mut usize,
) -> bool {
    loop {
        let Some(node) = super::transparent::transparent(value, remaining) else {
            return false;
        };
        if node.kind() != "identifier" {
            return false;
        }
        let name = node_text(content, node);
        if name == binding {
            return true;
        }
        let Some(previous) = preceding_value(content, node, &name, remaining) else {
            return false;
        };
        value = previous;
    }
}

pub(super) fn preceding_value<'a>(
    content: &str,
    mut position: Node<'a>,
    name: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    while let Some(parent) = position.parent() {
        *remaining = remaining.checked_sub(1)?;
        if matches!(parent.kind(), "module" | "block") {
            let mut previous = position.prev_named_sibling();
            while let Some(mut statement) = previous {
                *remaining = remaining.checked_sub(1)?;
                let next = statement.prev_named_sibling();
                if statement.kind() == "if_statement" {
                    let condition = super::transparent::transparent(
                        statement.child_by_field_name("condition")?,
                        remaining,
                    )?;
                    if condition.kind() != "true"
                        || statement.child_by_field_name("alternative").is_some()
                    {
                        return None;
                    }
                    let body = statement.child_by_field_name("consequence")?;
                    statement = body.named_child(
                        u32::try_from(body.named_child_count().checked_sub(1)?).ok()?,
                    )?;
                    previous = Some(statement);
                    continue;
                }
                match statement.kind() {
                    "comment" | "pass_statement" => {}
                    "function_definition" | "class_definition" => {
                        if statement
                            .child_by_field_name("name")
                            .is_some_and(|n| node_text(content, n) == name)
                        {
                            return Some(statement);
                        }
                        if statement.kind() != "function_definition"
                            || statement.child_by_field_name("return_type").is_some()
                        {
                            return None;
                        }
                        let parameters = statement.child_by_field_name("parameters")?;
                        for index in 0..parameters.named_child_count() {
                            *remaining = remaining.checked_sub(1)?;
                            if parameters.named_child(u32::try_from(index).ok()?)?.kind()
                                != "identifier"
                            {
                                return None;
                            }
                        }
                    }
                    "import_statement" | "import_from_statement" => {
                        let module = statement.child_by_field_name("module_name");
                        if module.is_some_and(|n| {
                            !matches!(
                                node_text(content, n).as_str(),
                                "typing" | "typing_extensions"
                            )
                        }) {
                            return None;
                        }
                        let mut cursor = statement.walk();
                        for imported in statement.children_by_field_name("name", &mut cursor) {
                            *remaining = remaining.checked_sub(1)?;
                            let original = imported.child_by_field_name("name").unwrap_or(imported);
                            let local = imported.child_by_field_name("alias").unwrap_or(original);
                            if node_text(content, local) == name {
                                return Some(statement);
                            }
                            if module.is_none()
                                && !matches!(
                                    node_text(content, original).as_str(),
                                    "typing" | "typing_extensions"
                                )
                            {
                                return None;
                            }
                        }
                        let mut cursor = statement.walk();
                        for child in statement.named_children(&mut cursor) {
                            *remaining = remaining.checked_sub(1)?;
                            if child.kind() == "wildcard_import" {
                                return None;
                            }
                        }
                    }
                    "expression_statement" => {
                        let mut assignment = statement.named_child(0)?;
                        if assignment.kind() != "assignment" {
                            return None;
                        }
                        let mut matched = false;
                        loop {
                            *remaining = remaining.checked_sub(1)?;
                            let target = assignment.child_by_field_name("left")?;
                            if target.kind() != "identifier" {
                                return None;
                            }
                            matched |= node_text(content, target) == name;
                            let value = super::transparent::transparent(
                                assignment.child_by_field_name("right")?,
                                remaining,
                            )?;
                            if value.kind() == "assignment" {
                                assignment = value;
                                continue;
                            }
                            if matched {
                                return Some(value);
                            }
                            if !matches!(
                                value.kind(),
                                "identifier"
                                    | "integer"
                                    | "float"
                                    | "true"
                                    | "false"
                                    | "none"
                                    | "string"
                            ) && !(matches!(
                                value.kind(),
                                "list" | "dictionary" | "tuple" | "set"
                            ) && value.named_child_count() == 0)
                            {
                                return None;
                            }
                            break;
                        }
                    }
                    _ => return None,
                }
                previous = next;
            }
            // A positive proof never guesses across a lexical/control boundary.
            return None;
        }
        position = parent;
    }
    None
}

pub(super) fn plain_instance(content: &str, receiver: Node<'_>, remaining: &mut usize) -> bool {
    let Some(value) = preceding_value(content, receiver, &node_text(content, receiver), remaining)
    else {
        return false;
    };
    if value.kind() != "call"
        || value
            .child_by_field_name("arguments")
            .is_none_or(|args| args.named_child_count() != 0)
    {
        return false;
    }
    let Some(constructor) = value
        .child_by_field_name("function")
        .filter(|n| n.kind() == "identifier")
    else {
        return false;
    };
    let Some(class) = preceding_value(
        content,
        constructor,
        &node_text(content, constructor),
        remaining,
    ) else {
        return false;
    };
    if class.kind() != "class_definition" || class.child_by_field_name("superclasses").is_some() {
        return false;
    }
    let Some(body) = class.child_by_field_name("body") else {
        return false;
    };
    for index in 0..body.named_child_count() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        let Some(statement) = body.named_child(u32::try_from(index).unwrap_or(u32::MAX)) else {
            return false;
        };
        if !matches!(statement.kind(), "pass_statement" | "comment") {
            return false;
        }
    }
    true
}

pub(super) fn imported_function(
    content: &str,
    receiver: Node<'_>,
    remaining: &mut usize,
    origins: crate::code::python_imports::PythonModuleOrigins,
) -> bool {
    let name = node_text(content, receiver);
    let Some(import) = preceding_value(content, receiver, &name, remaining) else {
        return false;
    };
    if import.kind() != "import_from_statement"
        || !import
            .child_by_field_name("module_name")
            .is_some_and(|n| origins.permits_standard_module(&node_text(content, n)))
    {
        return false;
    }
    let mut cursor = import.walk();
    for imported in import.children_by_field_name("name", &mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        let original = imported.child_by_field_name("name").unwrap_or(imported);
        let local = imported.child_by_field_name("alias").unwrap_or(original);
        if node_text(content, local) == name {
            return node_text(content, original) == "overload";
        }
    }
    false
}

#[cfg(test)]
#[path = "proven_aliases_tests.rs"]
mod tests;
