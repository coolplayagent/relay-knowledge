//! Bounded lexical receiver typing for configuration getter calls.
use super::{
    symbols,
    type_resolution::{is_type, text},
};
use tree_sitter::Node;

const MAX_LOOKUP_STEPS: usize = 4096;
const MAX_EXPRESSION_DEPTH: usize = 16;

pub(super) fn symbol(call: Node<'_>, content: &str) -> Option<String> {
    if call.child_by_field_name("arguments")?.named_child_count() != 0 {
        return None;
    }
    let name = text(call.child_by_field_name("name")?, content);
    if !name.starts_with("get") && !name.starts_with("is") {
        return None;
    }
    let mut budget = MAX_LOOKUP_STEPS;
    let owner = receiver_type(call.child_by_field_name("object")?, content, &mut budget, 0)?;
    Some(format!("{owner}.{name}"))
}

fn receiver_type(
    node: Node<'_>,
    content: &str,
    budget: &mut usize,
    depth: usize,
) -> Option<String> {
    *budget = budget.checked_sub(1)?;
    if depth >= MAX_EXPRESSION_DEPTH {
        return None;
    }
    match node.kind() {
        "identifier" => {
            let declaration = lexical_binding(node, text(node, content), content, budget)?;
            declared_type(declaration, content, budget, depth + 1)
        }
        "parenthesized_expression" => {
            receiver_type(node.named_child(0)?, content, budget, depth + 1)
        }
        "cast_expression" | "object_creation_expression" => {
            let ty = symbols::erased_type(node.child_by_field_name("type")?, content)?;
            Some(symbols::qualify(node, &ty, content))
        }
        "this" => {
            let owner = enclosing_type(node, budget)?;
            Some(symbols::qualify(
                owner,
                &symbols::type_owner(owner, content),
                content,
            ))
        }
        "field_access" => {
            let object = node.child_by_field_name("object")?;
            let owner = if object.kind() == "this" {
                enclosing_type(node, budget)?
            } else {
                let ty = receiver_type(object, content, budget, depth + 1)?;
                super::type_resolution::visible_parent(node, &ty, content, budget)?
            };
            let name = text(node.child_by_field_name("field")?, content);
            let declaration = field_binding(owner, name, content, budget)?;
            declared_type(declaration, content, budget, depth + 1)
        }
        _ => None,
    }
}

fn declared_type(
    node: Node<'_>,
    content: &str,
    budget: &mut usize,
    depth: usize,
) -> Option<String> {
    *budget = budget.checked_sub(1)?;
    let declaration = if node.kind() == "variable_declarator" {
        node.parent()?
    } else {
        node
    };
    let ty = declaration.child_by_field_name("type")?;
    if text(ty, content) == "var" {
        // Infer only from the declaration initializer, never from an arbitrary later value.
        return receiver_type(node.child_by_field_name("value")?, content, budget, depth);
    }
    // An array declarator is not an instance of its element type.
    if node.child_by_field_name("dimensions").is_some() {
        return None;
    }
    let ty = symbols::erased_type(ty, content)?;
    let head = ty.split('.').next()?;
    let mut scope = Some(declaration);
    while let Some(owner) = scope {
        *budget = budget.checked_sub(1)?;
        if let Some(parameters) = owner.child_by_field_name("type_parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                *budget = budget.checked_sub(1)?;
                let mut names = parameter.walk();
                if parameter
                    .named_children(&mut names)
                    .any(|name| name.kind() == "type_identifier" && text(name, content) == head)
                {
                    return None;
                }
            }
        }
        scope = owner.parent();
    }
    Some(symbols::qualify(declaration, &ty, content))
}

fn lexical_binding<'a>(
    mut node: Node<'a>,
    name: &str,
    content: &str,
    budget: &mut usize,
) -> Option<Node<'a>> {
    let position = node.start_byte();
    while let Some(parent) = node.parent() {
        *budget = budget.checked_sub(1)?;
        if matches!(
            parent.kind(),
            "block" | "for_statement" | "resource_specification"
        ) {
            let mut found = None;
            let mut cursor = parent.walk();
            for declaration in parent.named_children(&mut cursor) {
                *budget = budget.checked_sub(1)?;
                if declaration.start_byte() >= position {
                    break;
                }
                if matches!(
                    declaration.kind(),
                    "local_variable_declaration" | "resource"
                ) {
                    if let Some(binding) = named_binding(declaration, name, content, budget)? {
                        found = Some(binding);
                    }
                }
            }
            if found.is_some() {
                return found;
            }
        }
        if parent.kind() == "enhanced_for_statement"
            && parent.child_by_field_name("body") == Some(node)
            && named(parent, name, content)
        {
            return Some(parent);
        }
        if parent.kind() == "catch_clause" {
            let mut cursor = parent.walk();
            for parameter in parent.named_children(&mut cursor) {
                *budget = budget.checked_sub(1)?;
                if parameter.kind() == "catch_formal_parameter" && named(parameter, name, content) {
                    return Some(parameter);
                }
            }
        }
        if parent.kind() == "try_with_resources_statement"
            && parent.child_by_field_name("body") == Some(node)
        {
            if let Some(resources) = parent.child_by_field_name("resources") {
                let mut cursor = resources.walk();
                for resource in resources.named_children(&mut cursor) {
                    *budget = budget.checked_sub(1)?;
                    if let Some(binding) = named_binding(resource, name, content, budget)? {
                        return Some(binding);
                    }
                }
            }
        }
        if let Some(parameters) = parent.child_by_field_name("parameters") {
            if parameters.kind() == "identifier" && text(parameters, content) == name {
                return Some(parameters);
            }
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                *budget = budget.checked_sub(1)?;
                if named(parameter, name, content)
                    || (parameter.kind() == "identifier" && text(parameter, content) == name)
                {
                    return Some(parameter);
                }
            }
        }
        if is_type(parent) {
            if let Some(binding) = field_binding(parent, name, content, budget) {
                return Some(binding);
            }
            if *budget == 0 {
                return None;
            }
        }
        // Anonymous classes may hide a captured receiver through unknown inherited members.
        if parent.kind() == "object_creation_expression" {
            return None;
        }
        node = parent;
    }
    None
}

fn named_binding<'a>(
    node: Node<'a>,
    name: &str,
    content: &str,
    budget: &mut usize,
) -> Option<Option<Node<'a>>> {
    if named(node, name, content) {
        return Some(Some(node));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        *budget = budget.checked_sub(1)?;
        if child.kind() == "variable_declarator" && named(child, name, content) {
            return Some(Some(child));
        }
    }
    Some(None)
}

fn named(node: Node<'_>, name: &str, content: &str) -> bool {
    node.child_by_field_name("name")
        .is_some_and(|n| text(n, content) == name)
}

fn enclosing_type<'a>(mut node: Node<'a>, budget: &mut usize) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        *budget = budget.checked_sub(1)?;
        if is_type(parent) {
            return Some(parent);
        }
        if parent.kind() == "object_creation_expression" {
            return None;
        }
        node = parent;
    }
    None
}

fn field_binding<'a>(
    owner: Node<'a>,
    name: &str,
    content: &str,
    budget: &mut usize,
) -> Option<Node<'a>> {
    let mut pending = vec![owner];
    let mut visited = std::collections::BTreeSet::new();
    let mut found = None;
    while let Some(ty) = pending.pop() {
        *budget = budget.checked_sub(1)?;
        if !visited.insert(ty.id()) {
            continue;
        }
        let body = ty.child_by_field_name("body")?;
        let mut cursor = body.walk();
        let mut own = None;
        for field in body.named_children(&mut cursor) {
            *budget = budget.checked_sub(1)?;
            if matches!(field.kind(), "field_declaration" | "constant_declaration") {
                if let Some(binding) = named_binding(field, name, content, budget)? {
                    own = Some(binding);
                }
            }
        }
        if let Some(binding) = own {
            if ty == owner {
                return Some(binding);
            }
            if super::inherited_members::modifier(binding.parent()?, "private", budget)? {
                *budget = 0;
                return None;
            }
            if found.replace(binding).is_some() {
                *budget = 0;
                return None;
            }
            continue;
        }
        let mut cursor = ty.walk();
        for child in ty.named_children(&mut cursor) {
            *budget = budget.checked_sub(1)?;
            if matches!(
                child.kind(),
                "superclass" | "super_interfaces" | "extends_interfaces"
            ) {
                for parent in super::type_resolution::supertype_names(child, content, budget)? {
                    let Some(parent) =
                        super::type_resolution::visible_parent(ty, &parent, content, budget)
                    else {
                        // Unknown inherited fields cannot justify falling back to an outer field.
                        *budget = 0;
                        return None;
                    };
                    pending.push(parent);
                }
            }
        }
    }
    found
}

#[cfg(test)]
#[path = "getter_receivers_tests.rs"]
mod tests;
