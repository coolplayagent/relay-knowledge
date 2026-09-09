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
                    && input
                        .content
                        .get(parent.byte_range())
                        .is_some_and(|text| text.starts_with("export "))
            }) =>
        {
            let Some((key, value)) = source.split_once('=') else {
                return Ok(None);
            };
            (key, Some(value.trim_matches(['\'', '"'])), "defines_config")
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
    let default = default.filter(|value| !value.contains(['$', '`', '\\', '{', '}']));
    record.metadata.default_value = default.map(str::to_owned);
    record.metadata.value_type = default.map(|value| value_type(value).to_owned());
    Ok(Some(record))
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
