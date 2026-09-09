//! Prove unqualified platform calls from static imports and visible method scopes.
use tree_sitter::Node;

use super::symbols::{is_type, text};

const MAX_RESOLUTION_NODES: usize = 1024;

pub(super) fn receiver(node: Node<'_>, method: &str, content: &str) -> Option<&'static str> {
    let expected = match method {
        "getenv" | "getProperty" => "java.lang.System",
        "getBoolean" => "java.lang.Boolean",
        _ => return None,
    };
    let mut remaining = MAX_RESOLUTION_NODES;
    if method_may_shadow(node, method, content, &mut remaining) {
        return None;
    }
    let mut explicit = false;
    let mut exact = false;
    let mut conflict = false;
    let mut wildcard = false;
    let mut unknown_wildcard = false;
    let tree = bounded_root(node, &mut remaining)?;
    let mut cursor = tree.walk();
    for import in tree.named_children(&mut cursor) {
        if remaining == 0 {
            return None;
        }
        remaining -= 1;
        if import.kind() != "import_declaration" {
            continue;
        }
        let mut cursor = import.walk();
        let mut is_static = false;
        for child in import.children(&mut cursor) {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            is_static |= child.kind() == "static";
        }
        if !is_static {
            continue;
        }
        let (path, glob) = import_path(import, content, &mut remaining)?;
        if glob {
            wildcard |= path == expected;
            unknown_wildcard |= !matches!(path.as_str(), "java.lang.System" | "java.lang.Boolean");
        } else if let Some((owner, name)) = path.rsplit_once('.') {
            if name == method {
                explicit = true;
                exact |= owner == expected;
                conflict |= owner != expected;
            }
        }
    }
    if (explicit && exact && !conflict) || (!explicit && wildcard && !unknown_wildcard) {
        Some(expected)
    } else {
        None
    }
}

fn import_path(node: Node<'_>, content: &str, remaining: &mut usize) -> Option<(String, bool)> {
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

fn method_may_shadow(
    mut node: Node<'_>,
    method: &str,
    content: &str,
    remaining: &mut usize,
) -> bool {
    let mut types = Vec::new();
    while let Some(parent) = node.parent() {
        if *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        if matches!(
            parent.kind(),
            "class_body" | "interface_body" | "enum_body" | "enum_body_declarations"
        ) {
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if *remaining == 0 {
                    return true;
                }
                *remaining -= 1;
                if method_named(child, method, content) {
                    return true;
                }
            }
            if parent
                .parent()
                .is_some_and(|owner| owner.kind() == "object_creation_expression")
            {
                return true;
            }
        }
        if is_type(parent) {
            types.push(parent);
        }
        node = parent;
    }
    let mut visited = std::collections::BTreeSet::new();
    while let Some(owner) = types.pop() {
        if *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        if !visited.insert(owner.start_byte()) {
            continue;
        }
        let Some(body) = owner.child_by_field_name("body") else {
            return true;
        };
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if *remaining == 0 {
                return true;
            }
            *remaining -= 1;
            if method_named(child, method, content) {
                return true;
            }
        }
        let mut cursor = owner.walk();
        for declaration in owner.named_children(&mut cursor) {
            if *remaining == 0 {
                return true;
            }
            *remaining -= 1;
            if !matches!(
                declaration.kind(),
                "superclass" | "super_interfaces" | "extends_interfaces"
            ) {
                continue;
            }
            let Some(names) = supertype_names(declaration, content, remaining) else {
                return true;
            };
            for name in names {
                if name == "java.lang.Object" {
                    continue;
                }
                match visible_parent(owner, &name, content, remaining) {
                    Some(parent) => types.push(parent),
                    None if name == "Object" && implicit_object(owner, content, remaining) => {}
                    None => return true,
                }
            }
        }
    }
    false
}

fn method_named(node: Node<'_>, method: &str, content: &str) -> bool {
    node.kind() == "method_declaration"
        && node
            .child_by_field_name("name")
            .is_some_and(|name| text(name, content) == method)
}

fn supertype_names(node: Node<'_>, content: &str, remaining: &mut usize) -> Option<Vec<String>> {
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
fn bounded_root<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
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

fn visible_parent<'a>(
    mut scope: Node<'a>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    let position = scope.start_byte();
    let leaf = name.rsplit('.').next()?;
    loop {
        *remaining = remaining.checked_sub(1)?;
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
                if is_type(child)
                    && child
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == leaf)
                    && (scope.kind() != "block" || child.start_byte() <= position)
                {
                    return qualified_parent(child, name, content, remaining);
                }
            }
        }
        scope = scope.parent()?;
    }
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

fn implicit_object(node: Node<'_>, content: &str, remaining: &mut usize) -> bool {
    let Some(root) = bounded_root(node, remaining) else {
        return false;
    };
    let mut cursor = root.walk();
    for import in root.named_children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        if import.kind() != "import_declaration" {
            continue;
        }
        let Some((path, glob)) = import_path(import, content, remaining) else {
            return false;
        };
        if (glob && path != "java.lang")
            || (path.ends_with(".Object") && path != "java.lang.Object")
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "platform_imports_tests.rs"]
mod tests;
