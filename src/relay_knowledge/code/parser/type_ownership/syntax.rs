//! Language-specific syntax boundaries for the common ownership extractor.
use super::*;

pub(super) fn supports_types(language: &str) -> bool {
    matches!(
        language,
        "java"
            | "python"
            | "javascript"
            | "jsx"
            | "typescript"
            | "tsx"
            | "cpp"
            | "csharp"
            | "rust"
            | "go"
            | "kotlin"
            | "scala"
            | "ruby"
            | "php"
            | "swift"
    )
}

pub(super) fn is_type(language: &str, kind: &str) -> bool {
    match language {
        "go" => kind == "type_spec",
        "ruby" => matches!(kind, "class" | "module"),
        _ => matches!(
            kind,
            "class_declaration"
                | "class_definition"
                | "class_specifier"
                | "struct_specifier"
                | "struct_declaration"
                | "struct_item"
                | "enum_item"
                | "enum_declaration"
                | "enum_definition"
                | "enum_specifier"
                | "interface_declaration"
                | "trait_declaration"
                | "trait_definition"
                | "trait_item"
                | "protocol_declaration"
                | "object_declaration"
                | "object_definition"
                | "companion_object"
                | "record_declaration"
        ),
    }
}

pub(super) fn is_callable(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "method_declaration"
            | "method_definition"
            | "constructor_declaration"
            | "method"
            | "singleton_method"
            | "lambda"
            | "lambda_expression"
            | "arrow_function"
            | "anonymous_function"
            | "function_expression"
            | "function"
            | "protocol_function_declaration"
            | "init_declaration"
            | "deinit_declaration"
            | "subscript_declaration"
    )
}

pub(super) fn is_callable_field(node: Node<'_>, language: &str) -> bool {
    matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
        && matches!(node.kind(), "public_field_definition" | "field_definition")
        && node.child_by_field_name("value").is_some_and(|value| {
            matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function"
            )
        })
}

pub(super) fn is_namespace(kind: &str) -> bool {
    matches!(
        kind,
        "namespace_definition" | "namespace_declaration" | "mod_item"
    )
}

pub(super) fn name(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "companion_object" && node.child_by_field_name("name").is_none() {
        return Some("Companion".into());
    }
    let name = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find(|n| {
            matches!(
                n.kind(),
                "type_identifier" | "simple_identifier" | "user_type"
            )
        })
    })?;
    if matches!(node.kind(), "class_specifier" | "struct_specifier") {
        return templates::name(name, node, source);
    }
    type_name(name, source)
}

pub(super) fn visibility(node: Node<'_>, source: &str, language: &str) -> Option<String> {
    if !matches!(language, "rust" | "swift") || !is_type(language, node.kind()) {
        return None;
    }
    let mut cursor = node.walk();
    let modifier = node.named_children(&mut cursor).take(32).find_map(|child| {
        if child.kind() == "visibility_modifier" {
            return Some(child);
        }
        if child.kind() == "modifiers" {
            let mut cursor = child.walk();
            return child
                .named_children(&mut cursor)
                .take(32)
                .find(|n| n.kind() == "visibility_modifier");
        }
        None
    });
    let raw = modifier
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or_default();
    Some(
        match raw {
            "pub" | "public" | "open" => "public",
            "pub(crate)" => "crate",
            _ => "restricted",
        }
        .into(),
    )
}

pub(super) fn type_name(mut node: Node<'_>, source: &str) -> Option<String> {
    for depth in 0..16 {
        let Some(base) = node.child_by_field_name("type") else {
            break;
        };
        if depth == 15 {
            return None;
        }
        node = base;
    }
    let raw = node.utf8_text(source.as_bytes()).ok()?.trim();
    let raw = raw.trim_start_matches(['*', '&']).trim();
    if raw.len() > 1024 || raw.is_empty() || raw.contains(['<', '(', '[', ' ']) {
        return None;
    }
    Some(raw.replace("::", ".").replace('\\', "."))
}

pub(super) fn detached_owner(
    node: Node<'_>,
    source: &str,
    language: &str,
) -> Option<(String, &'static str)> {
    if language == "kotlin" && node.kind() == "function_declaration" {
        let name = node.child_by_field_name("name")?;
        let mut cursor = node.walk();
        if let Some(receiver) = node.named_children(&mut cursor).find(|n| {
            matches!(n.kind(), "type" | "user_type")
                && n.end_byte() < name.start_byte()
                && source[n.end_byte()..name.start_byte()].trim() == "."
        }) {
            return type_name(receiver, source).map(|name| (name, "direct_member"));
        }
    }
    if language == "go" && node.kind() == "method_declaration" {
        let receiver = node.child_by_field_name("receiver")?;
        let parameter = receiver.named_child(0)?;
        return type_name(parameter.child_by_field_name("type")?, source)
            .map(|n| (n, "direct_member"));
    }
    if language == "cpp" && node.kind() == "function_definition" {
        let declarator = node.child_by_field_name("declarator")?;
        let qualified = declarator.child_by_field_name("declarator")?;
        if qualified.kind() != "qualified_identifier" {
            return None;
        }
        let mut owner_parts = Vec::new();
        let mut current = qualified;
        for depth in 0..16 {
            let scope = current.child_by_field_name("scope")?;
            owner_parts.push(if scope.kind() == "template_type" {
                templates::name(scope, node, source)?
            } else {
                type_name(scope, source)?
            });
            let next = current.child_by_field_name("name")?;
            if next.kind() != "qualified_identifier" {
                return Some((owner_parts.join("."), "direct_member"));
            }
            if depth == 15 {
                return None;
            }
            current = next;
        }
    }
    None
}

pub(super) fn module_identity(
    root: Node<'_>,
    source: &str,
    path: &str,
    language: &str,
) -> Result<String, CodeIndexError> {
    let family = match language {
        "jsx" => "javascript",
        "tsx" => "typescript",
        other => other,
    };
    let mut package = String::new();
    let mut cursor = root.walk();
    for (count, child) in root.named_children(&mut cursor).enumerate() {
        if count >= MAX_NODES {
            return Err(incomplete("module declaration budget exceeded"));
        }
        if matches!(
            child.kind(),
            "package_declaration"
                | "package_header"
                | "package_clause"
                | "file_scoped_namespace_declaration"
        ) {
            let raw = child.utf8_text(source.as_bytes()).unwrap_or_default();
            package = raw
                .trim()
                .trim_end_matches([';', '{'])
                .trim()
                .strip_prefix("package ")
                .or_else(|| raw.trim().trim_end_matches(';').strip_prefix("namespace "))
                .unwrap_or_default()
                .trim()
                .to_owned();
        }
    }
    let boundary = match language {
        "java" | "kotlin" | "scala" | "cpp" | "csharp" | "php" | "swift" => package,
        "go" => format!(
            "{}:{package}",
            path.rsplit_once('/').map_or("", |(dir, _)| dir)
        ),
        "rust" => {
            let path = path.strip_suffix(".rs").unwrap_or(path);
            path.strip_suffix("/mod")
                .or_else(|| path.strip_suffix("/lib"))
                .or_else(|| path.strip_suffix("/main"))
                .unwrap_or(path)
                .to_owned()
        }
        _ => path.to_owned(),
    };
    Ok(format!("{family}|{boundary}"))
}
