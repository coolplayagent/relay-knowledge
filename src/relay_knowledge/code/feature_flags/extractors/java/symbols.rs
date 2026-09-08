//! Snapshot-stable Java symbol names for configuration constant/getter bindings.

use tree_sitter::Node;

pub(super) fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    content.get(node.byte_range()).unwrap_or_default()
}

pub(super) fn root(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

pub(super) fn qualify(node: Node<'_>, name: &str, content: &str) -> String {
    let mut parts = name.splitn(2, '.');
    let owner = parts.next().unwrap_or_default();
    let suffix = parts.next();
    let root = root(node);
    let mut package = "";
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "package_declaration" {
            package = text(child, content)
                .trim_start_matches("package")
                .trim()
                .trim_end_matches(';')
                .trim();
        }
        if child.kind() == "import_declaration" {
            let import = text(child, content)
                .trim_start_matches("import")
                .trim()
                .trim_start_matches("static ")
                .trim_end_matches(';')
                .trim();
            if import.rsplit('.').next() == Some(owner) {
                return suffix
                    .map_or_else(|| import.to_owned(), |suffix| format!("{import}.{suffix}"));
            }
        }
    }
    if package.is_empty() || owner.chars().next().is_some_and(char::is_lowercase) {
        name.to_owned()
    } else {
        format!("{package}.{name}")
    }
}

pub(super) fn enclosing<'a>(mut node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return Some(parent);
        }
        node = parent;
    }
    None
}

pub(super) fn constant_symbol(node: Node<'_>, content: &str) -> Option<String> {
    let value = text(node, content);
    match node.kind() {
        "field_access" => {
            let receiver = value.split('.').next()?;
            (!value_shadowed(node, receiver, content, true)).then(|| qualify(node, value, content))
        }
        "identifier" => {
            if value_shadowed(node, value, content, false) {
                return None;
            }
            let tree = root(node);
            let mut cursor = tree.walk();
            for child in tree.named_children(&mut cursor) {
                let import = text(child, content).trim();
                if child.kind() == "import_declaration" && import.starts_with("import static ") {
                    let name = import
                        .trim_start_matches("import static ")
                        .trim_end_matches(';')
                        .trim();
                    if name.rsplit('.').next() == Some(value) {
                        return Some(name.to_owned());
                    }
                }
            }
            let class = enclosing(node, "class_declaration")?;
            let body = class.child_by_field_name("body")?;
            let mut cursor = body.walk();
            let declared = body.named_children(&mut cursor).any(|field| {
                if field.kind() != "field_declaration" {
                    return false;
                }
                let mut cursor = field.walk();
                field.named_children(&mut cursor).any(|declarator| {
                    declarator.kind() == "variable_declarator"
                        && declarator
                            .child_by_field_name("name")
                            .is_some_and(|name| text(name, content) == value)
                })
            });
            if !declared {
                return None;
            }
            let owner = text(class.child_by_field_name("name")?, content);
            Some(qualify(node, &format!("{owner}.{value}"), content))
        }
        _ => None,
    }
}

pub(super) fn getter_symbol(node: Node<'_>, content: &str) -> Option<String> {
    if node.child_by_field_name("arguments")?.named_child_count() != 0 {
        return None;
    }
    let method = text(node.child_by_field_name("name")?, content);
    if !method.starts_with("get") && !method.starts_with("is") {
        return None;
    }
    let receiver = text(node.child_by_field_name("object")?, content);
    let declaration = enclosing(node, "method_declaration")?;
    let parameters = declaration.child_by_field_name("parameters")?;
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        let name = parameter
            .child_by_field_name("name")
            .map(|name| text(name, content));
        if name == Some(receiver) {
            let owner = text(parameter.child_by_field_name("type")?, content);
            return Some(qualify(node, &format!("{owner}.{method}"), content));
        }
    }
    None
}

pub(super) fn platform_receiver_shadowed(node: Node<'_>, receiver: &str, content: &str) -> bool {
    if value_shadowed(node, receiver, content, true) {
        return true;
    }
    let root = root(node);
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "class_declaration"
            && child
                .child_by_field_name("name")
                .is_some_and(|name| text(name, content) == receiver)
        {
            return true;
        }
        if child.kind() == "import_declaration" {
            let import = text(child, content)
                .trim_start_matches("import")
                .trim()
                .trim_end_matches(';');
            if import.rsplit('.').next() == Some(receiver) && !import.starts_with("java.lang.") {
                return true;
            }
        }
    }
    if let Some(method) = enclosing(node, "method_declaration") {
        if let Some(parameters) = method.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            if parameters.named_children(&mut cursor).any(|parameter| {
                parameter
                    .child_by_field_name("name")
                    .is_some_and(|name| text(name, content) == receiver)
            }) {
                return true;
            }
        }
    }
    false
}

// Inspect only lexical ancestor scopes. Declarations in completed sibling blocks
// must not hide a platform class or constant at a later, unrelated call site.
fn value_shadowed(mut node: Node<'_>, name: &str, content: &str, fields: bool) -> bool {
    let position = node.start_byte();
    while let Some(parent) = node.parent() {
        if let Some(parameters) = parent.child_by_field_name("parameters") {
            if parameters.kind() == "identifier" && text(parameters, content) == name {
                return true;
            }
            let mut cursor = parameters.walk();
            if parameters.named_children(&mut cursor).any(|parameter| {
                named(parameter, name, content)
                    || (parameter.kind() == "identifier" && text(parameter, content) == name)
            }) {
                return true;
            }
        }
        if parent.kind() == "catch_clause" {
            let mut cursor = parent.walk();
            if parent.named_children(&mut cursor).any(|parameter| {
                parameter.kind() == "catch_formal_parameter" && named(parameter, name, content)
            }) {
                return true;
            }
        }
        if parent.kind() == "try_with_resources_statement"
            && ["body", "resources"].iter().any(|field| {
                parent.child_by_field_name(field).is_some_and(|scope| {
                    scope.start_byte() <= position && position < scope.end_byte()
                })
            })
        {
            if let Some(resources) = parent.child_by_field_name("resources") {
                let mut cursor = resources.walk();
                if resources.named_children(&mut cursor).any(|resource| {
                    resource.start_byte() <= position && named(resource, name, content)
                }) {
                    return true;
                }
            }
        }
        if matches!(
            parent.kind(),
            "block" | "for_statement" | "try_with_resources_statement"
        ) {
            let mut cursor = parent.walk();
            if parent.named_children(&mut cursor).any(|declaration| {
                declaration.start_byte() <= position
                    && matches!(
                        declaration.kind(),
                        "local_variable_declaration" | "resource"
                    )
                    && declares(declaration, name, content)
            }) {
                return true;
            }
        }
        if matches!(
            parent.kind(),
            "enhanced_for_statement" | "catch_formal_parameter"
        ) && named(parent, name, content)
        {
            return true;
        }
        if fields && parent.kind() == "class_body" {
            let mut cursor = parent.walk();
            if parent
                .named_children(&mut cursor)
                .any(|field| field.kind() == "field_declaration" && declares(field, name, content))
            {
                return true;
            }
        }
        node = parent;
    }
    false
}

fn named(node: Node<'_>, name: &str, content: &str) -> bool {
    node.child_by_field_name("name")
        .is_some_and(|declared| text(declared, content) == name)
}

fn declares(node: Node<'_>, name: &str, content: &str) -> bool {
    if named(node, name, content) {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|declarator| {
        declarator.kind() == "variable_declarator" && named(declarator, name, content)
    })
}

pub(super) fn getter_bindings(node: Node<'_>, content: &str) -> Vec<String> {
    let Some(method) = enclosing(node, "method_declaration") else {
        return Vec::new();
    };
    let Some(name) = method.child_by_field_name("name") else {
        return Vec::new();
    };
    let name = text(name, content);
    if !name.starts_with("get") && !name.starts_with("is") {
        return Vec::new();
    }
    // A returned read provides evidence for the getter contract. Calls performed
    // solely for logging or side effects do not define its returned value.
    if enclosing(node, "return_statement").is_none() {
        return Vec::new();
    }
    let Some(class) = enclosing(node, "class_declaration") else {
        return Vec::new();
    };
    let Some(owner) = class.child_by_field_name("name") else {
        return Vec::new();
    };
    let mut bindings = vec![qualify(
        node,
        &format!("{}.{name}", text(owner, content)),
        content,
    )];
    if let Some(interfaces) = class.child_by_field_name("interfaces") {
        let interfaces = text(interfaces, content)
            .trim_start_matches("implements")
            .trim();
        for interface in interfaces.split(',') {
            let interface = interface.trim();
            if interface
                .chars()
                .all(|ch| ch.is_alphanumeric() || matches!(ch, '.' | '_' | '$'))
            {
                bindings.push(qualify(node, &format!("{interface}.{name}"), content));
            }
        }
    }
    bindings
}

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;
