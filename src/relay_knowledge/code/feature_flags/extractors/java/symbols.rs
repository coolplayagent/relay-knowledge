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
    let visible = visible_type(node, owner, content).map(|ty| type_owner(ty, content));
    let lexical_name =
        visible.map(|owner| suffix.map_or(owner.clone(), |suffix| format!("{owner}.{suffix}")));
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
        if lexical_name.is_none() && child.kind() == "import_declaration" {
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
    let name = lexical_name.as_deref().unwrap_or(name);
    if package.is_empty()
        || (lexical_name.is_none() && owner.chars().next().is_some_and(char::is_lowercase))
    {
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
            let mut class = node.parent()?;
            while !is_type(class) {
                class = class.parent()?;
            }
            let body = class.child_by_field_name("body")?;
            let mut cursor = body.walk();
            let declared = body.named_children(&mut cursor).any(|field| {
                if !matches!(field.kind(), "field_declaration" | "constant_declaration") {
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
            let owner = type_owner(class, content);
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
            let owner = erased_type(parameter.child_by_field_name("type")?, content)?;
            return Some(qualify(node, &format!("{owner}.{method}"), content));
        }
    }
    None
}

pub(super) fn platform_receiver_shadowed(node: Node<'_>, receiver: &str, content: &str) -> bool {
    if value_shadowed(node, receiver, content, true)
        || visible_type(node, receiver, content).is_some()
    {
        return true;
    }
    let root = root(node);
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
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
        if fields
            && matches!(
                parent.kind(),
                "class_body" | "interface_body" | "enum_body" | "enum_body_declarations"
            )
        {
            let mut cursor = parent.walk();
            if parent.named_children(&mut cursor).any(|field| {
                matches!(field.kind(), "field_declaration" | "constant_declaration")
                    && declares(field, name, content)
            }) {
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
    if method
        .child_by_field_name("parameters")
        .is_none_or(|parameters| parameters.named_child_count() != 0)
    {
        return Vec::new();
    }
    let Some(name) = method.child_by_field_name("name") else {
        return Vec::new();
    };
    let name = text(name, content);
    if !name.starts_with("get") && !name.starts_with("is") {
        return Vec::new();
    }
    // A returned read provides evidence for the getter contract. Calls performed
    // solely for logging or side effects do not define its returned value.
    let mut current = node;
    let mut returned = false;
    while let Some(parent) = current.parent() {
        if parent == method {
            break;
        }
        if matches!(
            parent.kind(),
            "lambda_expression" | "method_declaration" | "constructor_declaration" | "class_body"
        ) {
            return Vec::new();
        }
        returned |= parent.kind() == "return_statement";
        current = parent;
    }
    if !returned {
        return Vec::new();
    }
    let Some(class) = std::iter::successors(method.parent(), |parent| parent.parent())
        .find(|parent| is_type(*parent))
    else {
        return Vec::new();
    };
    let owner = type_owner(class, content);
    let mut bindings = vec![qualify(node, &format!("{owner}.{name}"), content)];
    if let Some(interfaces) = class.child_by_field_name("interfaces") {
        let mut cursor = interfaces.walk();
        for list in interfaces.named_children(&mut cursor) {
            let mut types = list.walk();
            for interface in list
                .named_children(&mut types)
                .filter_map(|ty| erased_type(ty, content))
            {
                bindings.push(qualify(node, &format!("{interface}.{name}"), content));
            }
        }
    }
    bindings
}

fn is_type(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

fn type_owner(mut node: Node<'_>, content: &str) -> String {
    let mut owners = Vec::new();
    loop {
        if is_type(node) {
            if let Some(name) = node.child_by_field_name("name") {
                owners.push(text(name, content));
            }
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }
    owners.reverse();
    owners.join(".")
}

fn visible_type<'a>(mut scope: Node<'a>, name: &str, content: &str) -> Option<Node<'a>> {
    let position = scope.start_byte();
    loop {
        if is_type(scope) && named(scope, name, content) {
            return Some(scope);
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
            if let Some(found) = scope.named_children(&mut cursor).find(|child| {
                is_type(*child)
                    && named(*child, name, content)
                    && (scope.kind() != "block" || child.start_byte() <= position)
            }) {
                return Some(found);
            }
        }
        scope = scope.parent()?;
    }
}

// Erase only structured type arguments; commas inside nested generics never
// become interface separators and both declaration/read identities agree.
fn erased_type(node: Node<'_>, content: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" | "identifier" => Some(text(node, content).to_owned()),
        "generic_type" => erased_type(node.named_child(0)?, content),
        "scoped_type_identifier" => {
            let mut cursor = node.walk();
            let names = node
                .named_children(&mut cursor)
                .filter(|child| child.kind() != "type_arguments")
                .map(|child| erased_type(child, content))
                .collect::<Option<Vec<_>>>()?;
            (!names.is_empty()).then(|| names.join("."))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;
