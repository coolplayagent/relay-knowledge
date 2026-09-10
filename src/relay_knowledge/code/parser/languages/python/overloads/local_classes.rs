//! Structural contracts for locally constructed protocol-free instances.
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Empty,
    LiteralManager,
}

pub(super) fn classify(content: &str, node: Node<'_>, remaining: &mut usize) -> Option<Class> {
    if node.kind() != "class_definition"
        || node.child_by_field_name("superclasses").is_some()
        || node.child_by_field_name("type_parameters").is_some()
    {
        return None;
    }
    let body = node.child_by_field_name("body")?;
    let mut enter = false;
    let mut exit = false;
    let mut cursor = body.walk();
    for statement in body.named_children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        if matches!(statement.kind(), "pass_statement" | "comment") {
            continue;
        }
        let name = statement.child_by_field_name("name")?;
        let name = content.get(name.byte_range())?;
        let (seen, expected) = match name {
            "__enter__" => (&mut enter, "none"),
            "__exit__" => (&mut exit, "false"),
            _ => return None,
        };
        if *seen || !literal_method(statement, expected, remaining) {
            return None;
        }
        *seen = true;
    }
    match (enter, exit) {
        (false, false) => Some(Class::Empty),
        (true, true) => Some(Class::LiteralManager),
        _ => None,
    }
}

fn literal_method(node: Node<'_>, expected: &str, remaining: &mut usize) -> bool {
    if node.kind() != "function_definition"
        || node.child(0).is_some_and(|child| child.kind() == "async")
        || node.child_by_field_name("return_type").is_some()
        || node.child_by_field_name("type_parameters").is_some()
    {
        return false;
    }
    let Some(parameters) = node.child_by_field_name("parameters") else {
        return false;
    };
    let mut ordinary = 0;
    let mut variadic = false;
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        match parameter.kind() {
            "identifier" if !variadic => ordinary += 1,
            "list_splat_pattern"
                if !variadic
                    && parameter.named_child_count() == 1
                    && parameter
                        .named_child(0)
                        .is_some_and(|n| n.kind() == "identifier") =>
            {
                variadic = true;
            }
            _ => return false,
        }
    }
    let valid_parameters = if expected == "none" {
        ordinary == 1 && !variadic
    } else {
        (ordinary == 1 && variadic) || (ordinary == 4 && !variadic)
    };
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut returned = false;
    let mut cursor = body.walk();
    for statement in body.named_children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        if matches!(statement.kind(), "comment" | "pass_statement") {
            continue;
        }
        if returned || statement.kind() != "return_statement" || statement.named_child_count() != 1
        {
            return false;
        }
        let Some(value) = statement
            .named_child(0)
            .and_then(|n| super::transparent::transparent(n, remaining))
        else {
            return false;
        };
        if value.kind() != expected {
            return false;
        }
        returned = true;
    }
    valid_parameters && returned
}
