//! Bash AST-backed environment exports and reads; literal/comment scopes are excluded.

use super::{valid_key, value_type};
use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
use crate::domain::{CodeFeatureFlagRecord, DomainError};
use tree_sitter::Node;

pub(in crate::code) fn extract(
    input: &FeatureFlagFileInput<'_>,
    root: Node<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut records = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let skipped = matches!(node.kind(), "raw_string" | "comment" | "heredoc_body");
        if !skipped {
            if let Some(record) = node_record(input, node)? {
                records.push(record);
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(records);
            }
        }
    }
}

fn node_record(
    input: &FeatureFlagFileInput<'_>,
    node: Node<'_>,
) -> Result<Option<CodeFeatureFlagRecord>, DomainError> {
    let source = input.content.get(node.byte_range()).unwrap_or_default();
    let (key, default, edge) = match node.kind() {
        "variable_assignment"
            if node.parent().is_some_and(|parent| {
                parent.kind() == "declaration_command"
                    && matches!(
                        super::shell_bindings::declaration_binding(parent, input.content),
                        Some(super::shell_bindings::Binding::Exported)
                    )
            }) =>
        {
            let Some((key, value)) = source.split_once('=') else {
                return Ok(None);
            };
            (key, Some(value), "defines_config")
        }
        "expansion" => {
            let mut cursor = node.walk();
            let Some(parameter) = node.named_children(&mut cursor).find_map(|child| {
                if child.kind() == "variable_name" {
                    Some(child)
                } else if child.kind() == "subscript" {
                    child.child_by_field_name("name")
                } else {
                    None
                }
            }) else {
                return Ok(None);
            };
            let key = input
                .content
                .get(parameter.byte_range())
                .unwrap_or_default();
            let default = node.child_by_field_name("operator").and_then(|operator| {
                let op = input.content.get(operator.byte_range())?;
                if operator.start_byte() >= parameter.end_byte()
                    && matches!(op, "-" | ":-" | "=" | ":=")
                {
                    input
                        .content
                        .get(operator.end_byte()..node.end_byte().checked_sub(1)?)
                } else {
                    None
                }
            });
            (key, default, "reads_config")
        }
        "simple_expansion" => (source.trim_start_matches('$'), None, "reads_config"),
        _ => return Ok(None),
    };
    if !valid_key(key) {
        return Ok(None);
    }
    if edge == "reads_config" && !super::shell_bindings::environment_read(node, key, input.content)
    {
        return Ok(None);
    }
    let mut record = feature_flag_record_from_range(
        input,
        "env_var",
        key,
        edge,
        ConfigRange {
            byte_start: node.start_byte(),
            byte_end: node.end_byte(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
        },
        source,
    )?;
    record.metadata.source_format = "shell".to_owned();
    let default = if edge == "reads_config" {
        default.and_then(|value| static_fallback(value, quoted_context(node)?))
    } else {
        default.and_then(|value| static_fallback(value, false))
    };
    record.metadata.value_type = default.as_deref().map(|value| value_type(value).to_owned());
    record.metadata.default_value = default;
    Ok(Some(record))
}

// Single quotes inside a double-quoted parameter expansion are literal data.
fn quoted_context(mut node: Node<'_>) -> Option<bool> {
    for _ in 0..1024 {
        let Some(parent) = node.parent() else {
            return Some(false);
        };
        match parent.kind() {
            "string" => return Some(true),
            "command" | "variable_assignment" | "declaration_command" | "program" => {
                return Some(false);
            }
            _ => node = parent,
        }
    }
    None
}

fn static_fallback(source: &str, outer_double: bool) -> Option<String> {
    if source.len() > 65_536 || source.contains(['$', '`', '\\', '{', '}']) {
        return None;
    }
    let mut quote = None;
    let mut value = String::with_capacity(source.len());
    for (index, ch) in source.char_indices() {
        if index == 0 && ch == '~' && !outer_double {
            return None;
        }
        match (quote, ch) {
            (Some(active), current) if active == current => quote = None,
            (None, '"') => quote = Some('"'),
            (None, '\'') if !outer_double => quote = Some('\''),
            _ => value.push(ch),
        }
    }
    quote.is_none().then_some(value)
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
