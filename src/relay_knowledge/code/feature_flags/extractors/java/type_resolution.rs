//! Shared bounded Java type identity and ancestor resolution.
use tree_sitter::Node;

pub(super) fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    content.get(node.byte_range()).unwrap_or_default()
}

pub(super) fn is_type(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

pub(super) fn import_path(
    node: Node<'_>,
    content: &str,
    remaining: &mut usize,
) -> Option<(String, bool)> {
    let mut names = Vec::new();
    let mut glob = false;
    let mut cursor = node.walk();
    loop {
        if *remaining == 0 {
            return None;
        }
        *remaining -= 1;
        let current = cursor.node();
        if current.kind() == "identifier" {
            names.push(text(current, content));
        }
        glob |= matches!(current.kind(), "asterisk" | "*");
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Some((names.join("."), glob));
            }
        }
    }
}

pub(super) fn supertype_names(
    node: Node<'_>,
    content: &str,
    remaining: &mut usize,
) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    loop {
        if *remaining == 0 {
            return None;
        }
        *remaining -= 1;
        let current = cursor.node();
        if matches!(
            current.kind(),
            "type_identifier" | "scoped_type_identifier" | "generic_type"
        ) {
            names.push(type_path(current, content, remaining)?);
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Some(names);
            }
        }
    }
}

// Every scan in this proof shares the caller's remaining budget. Existing general
// symbol qualification helpers intentionally stay outside this bounded owner.
pub(super) fn bounded_root<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        *remaining = remaining.checked_sub(1)?;
        node = parent;
    }
    Some(node)
}

fn type_path(node: Node<'_>, content: &str, remaining: &mut usize) -> Option<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    loop {
        *remaining = remaining.checked_sub(1)?;
        let current = cursor.node();
        if matches!(current.kind(), "type_identifier" | "identifier") {
            names.push(text(current, content));
        } else if current.kind() != "type_arguments" && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return (!names.is_empty()).then(|| names.join("."));
            }
        }
    }
}

pub(super) fn visible_parent<'a>(
    mut scope: Node<'a>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    let position = scope.start_byte();
    let leaf = name.rsplit('.').next()?;
    let relative = name.split_once('.');
    loop {
        *remaining = remaining.checked_sub(1)?;
        if let Some((head, tail)) = relative {
            if is_type(scope)
                && scope
                    .child_by_field_name("name")
                    .is_some_and(|node| text(node, content) == head)
            {
                return member_type_path(scope, tail, content, remaining);
            }
        }
        if is_type(scope)
            && scope
                .child_by_field_name("name")
                .is_some_and(|n| text(n, content) == leaf)
        {
            return qualified_parent(scope, name, content, remaining);
        }
        if matches!(
            scope.kind(),
            "program"
                | "class_body"
                | "interface_body"
                | "enum_body"
                | "enum_body_declarations"
                | "block"
        ) {
            let mut cursor = scope.walk();
            for child in scope.named_children(&mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                if let Some((head, tail)) = relative {
                    if is_type(child)
                        && child
                            .child_by_field_name("name")
                            .is_some_and(|node| text(node, content) == head)
                        && (scope.kind() != "block" || child.start_byte() <= position)
                    {
                        // Resolve from the nearest visible first segment. A local
                        // type hiding that segment must not fall back to another owner.
                        return member_type_path(child, tail, content, remaining);
                    }
                }
                if is_type(child)
                    && child
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == leaf)
                    && (scope.kind() != "block" || child.start_byte() <= position)
                {
                    if let Some(found) = qualified_parent(child, name, content, remaining) {
                        return Some(found);
                    }
                }
                if is_type(child)
                    && name.contains('.')
                    && (scope.kind() != "block" || child.start_byte() <= position)
                {
                    if let Some(found) = nested_parent(child, name, content, remaining) {
                        return Some(found);
                    }
                }
            }
        }
        scope = scope.parent()?;
    }
}

fn member_type_path<'a>(
    mut owner: Node<'a>,
    path: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    for segment in path.split('.') {
        *remaining = remaining.checked_sub(1)?;
        let mut bodies = vec![owner.child_by_field_name("body")?];
        let mut matched = None;
        while let Some(body) = bodies.pop() {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                if child.kind() == "enum_body_declarations" {
                    bodies.push(child);
                } else if is_type(child)
                    && child
                        .child_by_field_name("name")
                        .is_some_and(|node| text(node, content) == segment)
                    && matched.replace(child).is_some()
                {
                    return None;
                }
            }
        }
        owner = matched?;
    }
    Some(owner)
}

// Descend only through type bodies, never through methods or local classes.
// Validate the complete owner path so unrelated same-leaf types cannot match.
fn nested_parent<'a>(
    owner: Node<'a>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    let mut stack = vec![owner.child_by_field_name("body")?];
    while let Some(body) = stack.pop() {
        *remaining = remaining.checked_sub(1)?;
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            if child.kind() == "enum_body_declarations" {
                stack.push(child);
            } else if is_type(child) {
                if let Some(found) = qualified_parent(child, name, content, remaining) {
                    return Some(found);
                }
                if let Some(body) = child.child_by_field_name("body") {
                    stack.push(body);
                }
            }
        }
    }
    None
}

fn qualified_parent<'a>(
    node: Node<'a>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    if !name.contains('.') {
        return Some(node);
    }
    let mut scope = node;
    let mut owners = Vec::new();
    loop {
        *remaining = remaining.checked_sub(1)?;
        if is_type(scope) {
            owners.push(text(scope.child_by_field_name("name")?, content));
        }
        let Some(parent) = scope.parent() else {
            break;
        };
        scope = parent;
    }
    owners.reverse();
    let owner = owners.join(".");
    if owner == name {
        return Some(node);
    }
    let mut cursor = scope.walk();
    for declaration in scope.named_children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        if declaration.kind() == "package_declaration" {
            let (package, _) = import_path(declaration, content, remaining)?;
            return (format!("{package}.{owner}") == name).then_some(node);
        }
    }
    None
}

#[cfg(test)]
#[path = "type_resolution_tests.rs"]
mod tests;
