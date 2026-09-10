//! Java-only configuration facts; does not change the language call graph.
use super::*;
use tree_sitter::Node;
mod flow;
mod names;
mod static_imports;
use names::{field_symbol, literal, receiver_type, text};

pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(|e| DomainError::invalid("java", e.to_string()))?;
    let tree = parser
        .parse(input.content, None)
        .ok_or_else(|| DomainError::invalid("java", "parse cancelled"))?;
    let mut cursor = tree.root_node().walk();
    let mut rows = Vec::new();
    let mut methods = Vec::new();
    loop {
        let node = cursor.node();
        if node.kind() == "method_declaration"
            && node.child_by_field_name("body").is_some()
            && node
                .child_by_field_name("parameters")
                .is_some_and(|p| p.named_child_count() == 0)
        {
            methods.push(node);
        }
        if node.kind() == "method_invocation" {
            if let Some(mut row) = read(input, node)? {
                if let Some(method) = flow::returning_method(node, input.content) {
                    row.metadata.bindings = names::getter_bindings(method, input.content);
                } else if flow::inside_getter(node, input.content) {
                    row.metadata.flow_incomplete = Some("unsupported_getter_value_flow".into());
                }
                let guards = guard_sites(node, input.content);
                for guard in guards {
                    let mut usage = record(
                        input,
                        &row.source_kind,
                        &row.source_key,
                        "guards_code",
                        guard.start_byte(),
                        guard.end_byte(),
                    )?;
                    usage.metadata.reference.clone_from(&row.metadata.reference);
                    usage
                        .metadata
                        .target_kind
                        .clone_from(&row.metadata.target_kind);
                    usage.metadata.read_usage_id = Some(row.usage_id.clone());
                    rows.push(usage);
                }
                rows.push(row);
            }
        }
        if node.kind() == "variable_declarator"
            && node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "field_declaration" | "constant_declaration")
            })
        {
            let parent = node.parent().unwrap();
            let mut modifiers = parent.walk();
            let is_final = parent.kind() == "constant_declaration"
                || parent.named_children(&mut modifiers).any(|child| {
                    child.kind() == "modifiers"
                        && text(child, input.content)
                            .split_whitespace()
                            .any(|word| word == "final")
                });
            if is_final
                && parent.child_by_field_name("type").is_some_and(|ty| {
                    matches!(text(ty, input.content), "String" | "java.lang.String")
                })
            {
                if let (Some(name), Some(value)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("value"),
                ) {
                    if let Some(key) =
                        literal(value, input.content, 0).filter(|key| !key.is_empty())
                    {
                        let mut row = record(
                            input,
                            "config_key",
                            &key,
                            "declares_config_key",
                            node.start_byte(),
                            node.end_byte(),
                        )?;
                        row.metadata.bindings.push(field_symbol(
                            node,
                            text(name, input.content),
                            input.content,
                        ));
                        if !flow::key_declaration(node, input.content)
                            && row.metadata.domain.is_none()
                            && row.metadata.hot_reload.is_none()
                        {
                            row.edge_kind = "declares_string_constant".into();
                        }
                        rows.push(row);
                    }
                }
            }
        }
        if rows.len() > 10_000 {
            return Err(DomainError::invalid(
                "configuration",
                "file fact budget exceeded",
            ));
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                for method in methods {
                    let bindings = names::getter_bindings(method, input.content);
                    let Some(own) = bindings.first() else {
                        continue;
                    };
                    if rows.iter().any(|row| row.metadata.bindings.contains(own)) {
                        continue;
                    }
                    let mut marker = record(
                        input,
                        "config_symbol",
                        own,
                        "declares_config_getter",
                        method.start_byte(),
                        method.end_byte(),
                    )?;
                    marker.metadata.reference = Some(own.clone());
                    marker.metadata.bindings = bindings;
                    rows.push(marker);
                }
                return Ok(rows);
            }
        }
    }
}

fn read(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
) -> Result<Option<CodeFeatureFlagRecord>, DomainError> {
    let Some(name) = node.child_by_field_name("name") else {
        return Ok(None);
    };
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return Ok(None);
    };
    let method = text(name, input.content);
    let object = node.child_by_field_name("object");
    let platform = object
        .map(|object| text(object, input.content))
        .or_else(|| names::static_owner(node, method, input.content));
    let is_system = matches!(platform, Some("java.lang.System"))
        || (platform == Some("System") && names::platform_visible(node, "System", input.content));
    let is_boolean = matches!(platform, Some("java.lang.Boolean"))
        || (platform == Some("Boolean") && names::platform_visible(node, "Boolean", input.content));
    let kind = match method {
        "getProperty" if is_system => Some("config_key"),
        "getenv" if is_system => Some("env_var"),
        "getBoolean" if is_boolean => Some("config_key"),
        _ => None,
    };
    if let Some(kind) = kind {
        let count = arguments.named_child_count();
        if count == 0 || count > 2 || (method != "getProperty" && count != 1) {
            return Ok(None);
        }
        let argument = arguments.named_child(0).unwrap();
        let (source_kind, key, reference) = if let Some(key) = literal(argument, input.content, 0) {
            (kind, key, None)
        } else if let Some(reference) = names::key_symbol(argument, input.content) {
            ("config_symbol", reference.clone(), Some(reference))
        } else {
            return Ok(None);
        };
        if key.is_empty() {
            return Ok(None);
        }
        let mut row = record(
            input,
            source_kind,
            &key,
            "reads_config",
            node.start_byte(),
            node.end_byte(),
        )?;
        row.metadata.reference = reference;
        row.metadata.value_type = Some(
            if method == "getBoolean" {
                "boolean"
            } else {
                "string"
            }
            .to_owned(),
        );
        row.metadata.default_value = arguments
            .named_child(1)
            .and_then(|value| literal(value, input.content, 0));
        if method == "getBoolean" {
            row.metadata.default_value = Some("false".to_owned());
        }
        // Preserve the target namespace even when a constant supplies the key.
        if source_kind == "config_symbol" {
            row.metadata.target_kind = Some(kind.to_owned());
        }
        return Ok(Some(row));
    }
    if arguments.named_child_count() != 0
        || !(method.starts_with("get") || method.starts_with("is"))
    {
        return Ok(None);
    }
    let Some(object) = object else {
        return Ok(None);
    };
    let Some(owner) = receiver_type(object, input.content, 0) else {
        return Ok(None);
    };
    let key = format!("{owner}.{method}");
    let mut row = record(
        input,
        "config_symbol",
        &key,
        "reads_config",
        node.start_byte(),
        node.end_byte(),
    )?;
    row.metadata.reference = Some(key);
    Ok(Some(row))
}
fn guard_sites<'a>(node: Node<'a>, content: &str) -> Vec<Node<'a>> {
    let mut current = node;
    let mut guards = Vec::new();
    while let Some(parent) = current.parent() {
        if parent.child_by_field_name("condition") == Some(current)
            && matches!(
                parent.kind(),
                "if_statement"
                    | "while_statement"
                    | "for_statement"
                    | "do_statement"
                    | "ternary_expression"
            )
        {
            guards.push(current);
            return guards;
        }
        if parent.kind() == "variable_declarator"
            && parent.child_by_field_name("value") == Some(current)
        {
            let Some(name) = parent.child_by_field_name("name") else {
                return guards;
            };
            let Some(declaration) = parent
                .parent()
                .filter(|p| p.kind() == "local_variable_declaration")
            else {
                return guards;
            };
            let mut next = declaration.next_named_sibling();
            let mut budget = 2048;
            while let Some(statement) = next {
                let mut pending = vec![statement];
                while let Some(candidate) = pending.pop() {
                    if budget == 0 {
                        return guards;
                    }
                    budget -= 1;
                    if candidate.kind() == "block" {
                        let mut cursor = candidate.walk();
                        let shadows = candidate
                            .named_children(&mut cursor)
                            .filter(|n| n.kind() == "local_variable_declaration")
                            .any(|declaration| {
                                let mut cursor = declaration.walk();
                                declaration.named_children(&mut cursor).any(|variable| {
                                    variable.kind() == "variable_declarator"
                                        && variable.child_by_field_name("name").is_some_and(|n| {
                                            text(n, content) == text(name, content)
                                        })
                                })
                            });
                        if shadows {
                            continue;
                        }
                    }
                    let write = match candidate.kind() {
                        "assignment_expression" => candidate.child_by_field_name("left"),
                        "update_expression" => candidate.named_child(0),
                        "variable_declarator" => candidate.child_by_field_name("name"),
                        _ => None,
                    };
                    if write.is_some_and(|write| text(write, content) == text(name, content)) {
                        return guards;
                    }
                    if let Some(condition) = candidate.child_by_field_name("condition") {
                        if contains_name(condition, text(name, content), content, &mut budget) {
                            guards.push(condition);
                        }
                    }
                    if !matches!(
                        candidate.kind(),
                        "class_declaration"
                            | "class_body"
                            | "method_declaration"
                            | "lambda_expression"
                            | "constructor_declaration"
                    ) {
                        let mut cursor = candidate.walk();
                        let children = candidate.named_children(&mut cursor).collect::<Vec<_>>();
                        pending.extend(children.into_iter().rev());
                    }
                }
                next = statement.next_named_sibling();
            }
            return guards;
        }
        if matches!(
            parent.kind(),
            "statement"
                | "expression_statement"
                | "block"
                | "return_statement"
                | "method_declaration"
                | "lambda_expression"
        ) {
            return guards;
        }
        current = parent;
    }
    guards
}
fn contains_name(node: Node<'_>, name: &str, content: &str, budget: &mut usize) -> bool {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        if node.kind() == "identifier" && text(node, content) == name {
            return true;
        }
        if !matches!(node.kind(), "lambda_expression" | "class_body") {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    false
}
