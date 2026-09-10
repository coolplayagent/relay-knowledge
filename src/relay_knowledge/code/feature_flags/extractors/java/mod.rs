//! Java configuration reads and direct, lexically scoped guard dependencies.

use tree_sitter::Node;

use crate::domain::{CodeConfigurationReadKind, CodeFeatureFlagRecord, DomainError};

use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
mod getter_receivers;
mod inherited_constants;
mod inherited_members;
mod platform_imports;
mod string_defaults;
mod symbols;
mod type_resolution;
use symbols as java_symbols;

/// Uses the already parsed syntax tree; source strings and comments never become calls.
pub(in crate::code) fn extract(
    input: &FeatureFlagFileInput<'_>,
    root: Node<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut records = Vec::new();
    let namespace = crate::code::java_namespace::collect(root, input.content);
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "variable_declarator" {
            if let Some(definition) = constant_definition(input, node)? {
                records.push(definition);
            }
        }
        if node.kind() == "method_invocation" {
            if let Some((kind, key)) = config_read(node, input.content) {
                let mut usage = record(input, node, kind, &key, "reads_config")?;
                usage.metadata.bindings = java_symbols::getter_bindings(node, input.content);
                if matches!(kind, "config_symbol" | "config_getter") {
                    usage.metadata.referenced_symbol = Some(key.clone());
                }
                usage.metadata.default_value = default_value(node, input.content);
                usage.metadata.java_implicit_platform = node
                    .child_by_field_name("object")
                    .map(|object| text(object, input.content))
                    .filter(|name| {
                        matches!(*name, "System" | "Boolean")
                            && !namespace.explicit_platform_types.contains(*name)
                    })
                    .map(|name| crate::domain::JavaImplicitPlatformRead {
                        type_name: name.to_owned(),
                    });
                let method_name = text(
                    node.child_by_field_name("name").unwrap_or(node),
                    input.content,
                );
                usage.metadata.read_source_kind = match (kind, method_name) {
                    ("config_getter", _) => None,
                    (_, "getenv") => Some(CodeConfigurationReadKind::EnvVar),
                    (_, "getProperty" | "getBoolean") => Some(CodeConfigurationReadKind::ConfigKey),
                    _ => None,
                };
                usage.metadata.value_type = match (kind, method_name) {
                    ("config_getter", _) => None,
                    (_, "getBoolean") => Some("boolean".to_owned()),
                    (_, "getProperty" | "getenv") => Some("string".to_owned()),
                    _ => None,
                };
                let read_usage_id = usage.usage_id.clone();
                let implicit_platform = usage.metadata.java_implicit_platform.clone();
                let read_source_kind = usage.metadata.read_source_kind;
                records.push(usage);
                let guard_start = records.len();
                if let Some(guard) = containing_guard(node) {
                    records.push(record(input, guard, kind, &key, "guards_code")?);
                }
                collect_variable_guards(input, node, kind, &key, &mut records)?;
                for guard in &mut records[guard_start..] {
                    guard.usage_id = crate::code::stable_id(
                        "feature_flag_guard",
                        [guard.usage_id.as_str(), read_usage_id.as_str()],
                    );
                    guard.metadata.read_usage_id = Some(read_usage_id.clone());
                    guard.metadata.read_source_kind = read_source_kind;
                    guard.metadata.java_implicit_platform = implicit_platform.clone();
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(records);
            }
        }
    }
}

fn config_read(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    let name = text(node.child_by_field_name("name")?, content);
    let object = match node.child_by_field_name("object") {
        Some(object) => text(object, content),
        None => platform_imports::receiver(node, name, content)?,
    };
    let shadow_root = match object {
        "System" | "Boolean" => Some(object),
        "java.lang.System" | "java.lang.Boolean"
            if node.child_by_field_name("object").is_some() =>
        {
            Some("java")
        }
        _ => None,
    };
    if shadow_root.is_some_and(|root| java_symbols::platform_receiver_shadowed(node, root, content))
    {
        return None;
    }
    let kind = match (object, name) {
        ("System" | "java.lang.System", "getProperty")
        | ("Boolean" | "java.lang.Boolean", "getBoolean") => "config_key",
        ("System" | "java.lang.System", "getenv") => "env_var",
        _ => {
            return getter_receivers::symbol(node, content).map(|symbol| ("config_getter", symbol));
        }
    };
    let argument = node.child_by_field_name("arguments")?.named_child(0)?;
    if argument.kind() != "string_literal" {
        return java_symbols::constant_symbol(argument, content)
            .map(|symbol| ("config_symbol", symbol));
    }
    let key = string_defaults::decode(text(argument, content))?;
    if key.is_empty() {
        return None;
    }
    Some((kind, key))
}

fn constant_definition(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
) -> Result<Option<CodeFeatureFlagRecord>, DomainError> {
    let Some(field) = node
        .parent()
        .filter(|parent| matches!(parent.kind(), "field_declaration" | "constant_declaration"))
    else {
        return Ok(None);
    };
    let mut is_static = field.kind() == "constant_declaration";
    let mut is_final = is_static;
    if let Some(modifiers) = field
        .named_child(0)
        .filter(|node| node.kind() == "modifiers")
    {
        let mut cursor = modifiers.walk();
        for modifier in modifiers.children(&mut cursor) {
            is_static |= modifier.kind() == "static";
            is_final |= modifier.kind() == "final";
        }
    }
    if !is_static || !is_final {
        return Ok(None);
    }
    let Some(value) = node
        .child_by_field_name("value")
        .filter(|value| value.kind() == "string_literal")
    else {
        return Ok(None);
    };
    let Some(key) = string_defaults::decode(text(value, input.content)) else {
        return Ok(None);
    };
    if key.is_empty() {
        return Ok(None);
    }
    let Some(name) = node.child_by_field_name("name") else {
        return Ok(None);
    };
    let Some(symbol) = java_symbols::constant_symbol(name, input.content) else {
        return Ok(None);
    };
    // A string constant alone does not prove a configuration key. Persist only
    // a binding candidate; snapshot resolution promotes it when a real read or
    // independent configuration definition provides matching evidence.
    let mut definition = record(input, node, "config_key", &key, "binds_config_symbol")?;
    definition.metadata.bindings.push(symbol);
    Ok(Some(definition))
}

fn default_value(node: Node<'_>, content: &str) -> Option<String> {
    let argument = node.child_by_field_name("arguments")?.named_child(1)?;
    (argument.kind() == "string_literal")
        .then(|| string_defaults::decode(text(argument, content)))
        .flatten()
}

fn containing_guard(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "if_statement"
                | "while_statement"
                | "do_statement"
                | "ternary_expression"
                | "for_statement"
        ) {
            let condition = parent.child_by_field_name("condition")?;
            return (condition.start_byte() <= node.start_byte()
                && condition.end_byte() >= node.end_byte())
            .then_some(condition);
        }
        if matches!(
            parent.kind(),
            "block" | "method_declaration" | "lambda_expression"
        ) {
            return None;
        }
        node = parent;
    }
    None
}

fn collect_variable_guards(
    input: &FeatureFlagFileInput<'_>,
    mut read: Node<'_>,
    kind: &str,
    key: &str,
    records: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    while let Some(parent) = read.parent() {
        if parent.kind() == "variable_declarator" {
            let Some(name) = parent.child_by_field_name("name") else {
                return Ok(());
            };
            let Some(declaration) = parent.parent() else {
                return Ok(());
            };
            // Follow only subsequent statements of the same lexical block. Stop at
            // any write or nested scope; never assume interprocedural data flow.
            let mut next = declaration.next_named_sibling();
            while let Some(statement) = next {
                let write =
                    first_write_position(statement, text(name, input.content), input.content);
                collect_statement_guards(
                    input,
                    statement,
                    text(name, input.content),
                    kind,
                    key,
                    records,
                    write,
                )?;
                if write.is_some() {
                    break;
                }
                next = statement.next_named_sibling();
            }
            break;
        }
        if matches!(
            parent.kind(),
            "statement" | "block" | "method_declaration" | "lambda_expression" | "class_body"
        ) {
            break;
        }
        read = parent;
    }
    Ok(())
}

fn collect_statement_guards(
    input: &FeatureFlagFileInput<'_>,
    statement: Node<'_>,
    name: &str,
    kind: &str,
    key: &str,
    records: &mut Vec<CodeFeatureFlagRecord>,
    first_write: Option<usize>,
) -> Result<(), DomainError> {
    let mut cursor = statement.walk();
    loop {
        let node = cursor.node();
        let boundary = matches!(
            node.kind(),
            "block"
                | "lambda_expression"
                | "class_body"
                | "method_declaration"
                | "constructor_declaration"
        );
        if !boundary {
            if let Some(condition) = node.child_by_field_name("condition") {
                if first_write.is_none_or(|write| condition.end_byte() <= write)
                    && contains_identifier(condition, name, input.content)
                {
                    records.push(record(input, condition, kind, key, "guards_code")?);
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(());
            }
        }
    }
}

fn contains_identifier(node: Node<'_>, name: &str, content: &str) -> bool {
    let mut cursor = node.walk();
    loop {
        let current = cursor.node();
        if current.kind() == "identifier"
            && text(current, content) == name
            && !current.parent().is_some_and(|parent| {
                matches!(parent.kind(), "field_access" | "method_invocation")
                    && (parent.child_by_field_name("field") == Some(current)
                        || parent.child_by_field_name("name") == Some(current))
            })
        {
            return true;
        }
        if !matches!(
            current.kind(),
            "lambda_expression" | "class_body" | "method_declaration" | "constructor_declaration"
        ) && cursor.goto_first_child()
        {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}

fn first_write_position(node: Node<'_>, name: &str, content: &str) -> Option<usize> {
    let mut cursor = node.walk();
    loop {
        let current = cursor.node();
        let target = match current.kind() {
            "assignment_expression" => current.child_by_field_name("left"),
            "variable_declarator" => current.child_by_field_name("name"),
            "update_expression" => current.named_child(0),
            _ => None,
        };
        if target
            .is_some_and(|target| target.kind() == "identifier" && text(target, content) == name)
        {
            return Some(current.start_byte());
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

fn record(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
    kind: &str,
    key: &str,
    edge: &str,
) -> Result<CodeFeatureFlagRecord, DomainError> {
    let mut record = feature_flag_record_from_range(
        input,
        kind,
        key,
        edge,
        ConfigRange {
            byte_start: node.start_byte(),
            byte_end: node.end_byte(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
        },
        text(node, input.content),
    )?;
    if matches!(kind, "config_symbol" | "config_getter") {
        record.metadata.referenced_symbol = Some(key.to_owned());
    }
    Ok(record)
}

fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    content.get(node.byte_range()).unwrap_or_default()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
