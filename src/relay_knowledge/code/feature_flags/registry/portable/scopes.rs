//! Lexical identity and conservative reaching definitions for portable flow.
use super::*;

pub(super) fn is_type(kind: &str) -> bool {
    matches!(
        kind,
        "class_definition"
            | "class_declaration"
            | "class"
            | "module"
            | "struct_item"
            | "struct_declaration"
            | "object_declaration"
            | "object_definition"
            | "trait_definition"
            | "trait_item"
            | "namespace_definition"
            | "namespace_declaration"
    )
}

pub(super) fn lexical_identity(mut node: Node<'_>, source: &str) -> String {
    let mut parts = Vec::new();
    for _ in 0..128 {
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
        if is_type(node.kind()) {
            if let Some(name) = node.child_by_field_name("name") {
                parts.push(
                    syntax::text(name, source)
                        .replace("::", ".")
                        .replace('\\', "."),
                );
            }
        } else if syntax::is_function(node.kind()) {
            parts.push(format!("local@{}", node.start_byte()));
        }
    }
    parts.reverse();
    if let Some(namespace) = file_namespace(node, source) {
        parts.insert(0, namespace);
    }
    parts.join(".")
}

fn file_namespace(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "compilation_unit" {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .take(1024)
        .find(|child| child.kind() == "file_scoped_namespace_declaration")
        .and_then(|child| child.child_by_field_name("name"))
        .map(|name| syntax::text(name, source).to_owned())
}

/// C# namespace lookup starts in the enclosing namespace. A same-named global
/// type cannot stand in for an unproved type in this scope.
pub(super) fn csharp_namespace(mut node: Node<'_>, source: &str) -> String {
    let mut parts = Vec::new();
    for _ in 0..128 {
        if node.kind() == "namespace_declaration"
            && let Some(name) = node.child_by_field_name("name")
        {
            parts.push(syntax::text(name, source).to_owned());
        }
        let Some(parent) = node.parent() else { break };
        node = parent;
    }
    parts.reverse();
    if let Some(namespace) = file_namespace(node, source) {
        parts.insert(0, namespace);
    }
    parts.join(".")
}

fn enclosing_function(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..128 {
        node = node.parent()?;
        if syntax::is_function(node.kind()) {
            return Some(node);
        }
    }
    None
}

pub(super) fn parameter_shadows(node: Node<'_>, name: &str, source: &str) -> bool {
    let Some(function) = enclosing_function(node) else {
        return false;
    };
    let mut parameter_roots = Vec::new();
    let mut declaration = function;
    for _ in 0..8 {
        if let Some(parameters) = declaration.child_by_field_name("parameters") {
            parameter_roots.push(parameters);
        }
        let Some(next) = declaration.child_by_field_name("declarator") else {
            break;
        };
        declaration = next;
    }
    let mut cursor = function.walk();
    for child in function.named_children(&mut cursor).take(1025) {
        if matches!(
            child.kind(),
            "function_value_parameters" | "parameter" | "parameters"
        ) && !parameter_roots.contains(&child)
        {
            parameter_roots.push(child);
        }
    }
    let mut count = 0;
    for parameters in parameter_roots {
        let mut cursor = parameters.walk();
        loop {
            count += 1;
            if count > 1024 {
                return true;
            }
            if syntax::is_identifier(cursor.node().kind())
                && syntax::text(cursor.node(), source).trim_start_matches('$') == name
            {
                return true;
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    break;
                }
            }
            if cursor.node() == parameters {
                break;
            }
        }
    }
    false
}

impl Analysis<'_> {
    pub(super) fn stable_getter(&self, function: Node<'_>, name: &str) -> bool {
        let identity = lexical_identity(function, self.input.content);
        !self.side_effects.contains_key(name)
            && !self.declarations.get(name).is_some_and(|nodes| {
                nodes.iter().any(|node| {
                    node.id() != function.id()
                        && lexical_identity(*node, self.input.content) == identity
                })
            })
            && self
                .functions
                .get(name)
                .into_iter()
                .flatten()
                .copied()
                .chain(
                    self.properties
                        .get(name)
                        .into_iter()
                        .flatten()
                        .map(|(node, _)| *node),
                )
                .filter(|node| lexical_identity(*node, self.input.content) == identity)
                .map(|node| node.id())
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([function.id()])
    }
    pub(super) fn binding_shadowed(&self, use_site: Node<'_>, spelling: &str) -> bool {
        let root = spelling
            .split('.')
            .next()
            .unwrap_or(spelling)
            .trim_start_matches('$');
        parameter_shadows(use_site, root, self.input.content)
            || self.declarations.get(root).is_some_and(|nodes| {
                nodes
                    .iter()
                    .any(|node| syntax::visible_function(*node, use_site, root, self.input.content))
            })
            || self
                .side_effects
                .get(root)
                .is_some_and(|ranges| ranges.iter().any(|(_, end)| *end < use_site.start_byte()))
    }

    pub(super) fn reaching_value(&self, use_site: Node<'_>, name: &str) -> Option<Node<'_>> {
        if parameter_shadows(use_site, name, self.input.content) {
            return None;
        }
        let candidates = self.declarations.get(name)?;
        let mut eligible = candidates
            .iter()
            .copied()
            .filter(|candidate| syntax::visible(*candidate, use_site));
        let declaration = eligible.next()?;
        if eligible.next().is_some() {
            return None;
        }
        let identity = lexical_identity(declaration, self.input.content);
        // A later write in an enclosing scope may execute before a getter or
        // closure is called. Never export a guessed stable value in that case.
        if candidates.iter().any(|other| {
            other.id() != declaration.id()
                && lexical_identity(*other, self.input.content) == identity
        }) {
            return None;
        }
        let function = enclosing_function(declaration);
        if function.is_some() && function != enclosing_function(use_site) {
            return None;
        }
        let value = syntax::assignment(declaration, self.input.content)?.1;
        // Unknown calls invalidate flow through mutable locals if they receive
        // that binding (including a possible address/reference escape).
        if let Some(ranges) = self.side_effects.get(name) {
            let start = ranges.partition_point(|(start, _)| *start <= declaration.end_byte());
            if ranges[start..]
                .iter()
                .take_while(|(start, _)| *start < use_site.start_byte())
                .any(|(_, end)| *end < use_site.start_byte())
            {
                return None;
            }
        }
        Some(value)
    }

    pub(super) fn stable_constant(&self, node: Node<'_>, name: &str) -> bool {
        syntax::constant_declaration(node, self.input.language_id, self.input.content)
            && enclosing_function(node).is_none()
            && !self.side_effects.contains_key(name)
            && self.declarations.get(name).is_some_and(|nodes| {
                nodes
                    .iter()
                    .filter(|other| {
                        lexical_identity(**other, self.input.content)
                            == lexical_identity(node, self.input.content)
                    })
                    .count()
                    == 1
            })
    }
}

pub(super) fn export_status(mut node: Node<'_>, language: &str, source: &str) -> (bool, bool) {
    let js = matches!(language, "javascript" | "jsx" | "typescript" | "tsx");
    let mut public = !js && language != "rust" && language != "csharp";
    for _ in 0..8 {
        let mut cursor = node.walk();
        let mut modifiers = Vec::new();
        for child in node.named_children(&mut cursor).take(64) {
            if matches!(
                child.kind(),
                "visibility_modifier" | "access_modifier" | "modifier" | "storage_class_specifier"
            ) {
                modifiers.push(syntax::text(child, source));
            } else if matches!(child.kind(), "modifiers" | "modifiers_list") {
                let mut cursor = child.walk();
                for modifier in child.named_children(&mut cursor).take(32) {
                    if matches!(
                        modifier.kind(),
                        "visibility_modifier" | "access_modifier" | "modifier"
                    ) {
                        modifiers.push(syntax::text(modifier, source));
                    }
                }
            }
        }
        if modifiers
            .iter()
            .any(|modifier| matches!(*modifier, "private" | "fileprivate"))
            || (matches!(language, "c" | "cpp") && modifiers.contains(&"static"))
        {
            return (false, false);
        }
        if modifiers
            .iter()
            .any(|modifier| matches!(*modifier, "pub" | "pub(crate)" | "public" | "internal"))
        {
            public = true;
        }
        if node.kind() == "export_statement" {
            let mut cursor = node.walk();
            return (
                true,
                node.children(&mut cursor)
                    .take(8)
                    .any(|child| child.kind() == "default"),
            );
        }
        let Some(parent) = node.parent() else {
            break;
        };
        if is_type(parent.kind()) || syntax::is_function(parent.kind()) {
            break;
        }
        node = parent;
    }
    (public, false)
}
