use tree_sitter::Node;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) fn definition_kind(
    language_id: &str,
    node_kind: &str,
) -> Option<&'static str> {
    match language_id {
        "json" if node_kind == "pair" => Some("config"),
        "ini" if node_kind == "section" => Some("section"),
        "ini" if node_kind == "setting" => Some("config"),
        "toml" if matches!(node_kind, "table" | "table_array_element") => Some("section"),
        "toml" if node_kind == "pair" => Some("config"),
        "yaml" if matches!(node_kind, "block_mapping_pair" | "flow_pair") => Some("config"),
        "properties" if node_kind == "property" => Some("config"),
        _ => None,
    }
}

pub(in crate::code::parser) fn manual_definition_candidate(
    language_id: &str,
    node_kind: &str,
) -> bool {
    definition_kind(language_id, node_kind).is_some()
}

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match language_id {
        "ini" => return ini_definitions(content, node),
        "toml" => return toml_definitions(content, node),
        _ => {}
    }
    let Some(kind) = definition_kind(language_id, node.kind()) else {
        return Vec::new();
    };
    let Some(name) = (match language_id {
        "json" => json_pair_path(content, node),
        "yaml" => yaml_pair_path(content, node),
        "properties" => properties_key(content, node),
        _ => None,
    }) else {
        return Vec::new();
    };
    vec![(name, kind, syntax_range(node))]
}

fn json_pair_path(content: &str, node: Node<'_>) -> Option<String> {
    let key = json_pair_key(content, node)?;
    let mut parts = ancestor_pair_keys(content, node, &["pair"], json_pair_key, sequence_suffix);
    parts.push(key);
    Some(parts.join("."))
}

fn yaml_pair_path(content: &str, node: Node<'_>) -> Option<String> {
    let key = yaml_pair_key(content, node)?;
    let mut parts = ancestor_pair_keys(
        content,
        node,
        &["block_mapping_pair", "flow_pair"],
        yaml_pair_key,
        sequence_suffix,
    );
    parts.push(key);
    Some(parts.join("."))
}

fn toml_definitions(content: &str, node: Node<'_>) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "pair" => toml_pair_path(content, node)
            .map(|name| vec![(name, "config", syntax_range(node))])
            .unwrap_or_default(),
        "table" | "table_array_element" => toml_table_path(content, node)
            .map(|name| {
                vec![(
                    name,
                    "section",
                    first_key_node(node)
                        .map(syntax_range)
                        .unwrap_or_else(|| syntax_range(node)),
                )]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn toml_pair_path(content: &str, node: Node<'_>) -> Option<String> {
    let mut parts = toml_table_context(content, node);
    parts.extend(ancestor_pair_keys(
        content,
        node,
        &["pair"],
        toml_pair_key_text,
        sequence_suffix,
    ));
    parts.extend(toml_key_parts(content, first_key_node(node)?)?);
    (!parts.is_empty()).then(|| parts.join("."))
}

fn toml_table_path(content: &str, node: Node<'_>) -> Option<String> {
    let mut parts = toml_key_parts(content, first_key_node(node)?)?;
    mark_prior_toml_array_table_parts(content, node, &mut parts);
    if node.kind() == "table_array_element" {
        let current_prefix_len = parts.len();
        mark_toml_array_table_part(&mut parts, current_prefix_len);
    }
    (!parts.is_empty()).then(|| parts.join("."))
}

fn toml_table_context(content: &str, node: Node<'_>) -> Vec<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "table" | "table_array_element") {
            return toml_table_path(content, parent)
                .map(|name| name.split('.').map(str::to_owned).collect())
                .unwrap_or_default();
        }
        current = parent.parent();
    }

    Vec::new()
}

fn ini_definitions(content: &str, node: Node<'_>) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "section" => ini_section_definitions(content, node),
        "setting" => ini_setting_path(content, node)
            .map(|name| vec![(name, "config", syntax_range(node))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn ini_section_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let Some((section, range)) = ini_section_name(content, node) else {
        return Vec::new();
    };
    let mut definitions = vec![(section.clone(), "section", range)];
    definitions.extend(ini_section_setting_paths(content, node, &section));
    definitions
}

fn ini_setting_path(content: &str, node: Node<'_>) -> Option<String> {
    let key = child_text(content, node, "setting_name")?;
    let Some((section, _)) = nearest_ini_section(content, node) else {
        return Some(key);
    };

    Some(format!("{section}.{key}"))
}

fn ini_section_setting_paths(
    content: &str,
    node: Node<'_>,
    section: &str,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let Some(section_text) = content.get(node.end_byte()..) else {
        return Vec::new();
    };

    let mut definitions = Vec::new();
    let mut byte_start = node.end_byte();
    let mut line_number = syntax_range(node).line_end;
    for line in section_text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if let Some((key, range)) = ini_line_setting_key(line, byte_start, line_number) {
            definitions.push((format!("{section}.{key}"), "config", range));
        }
        byte_start += line.len();
        if line.ends_with('\n') {
            line_number += 1;
        }
    }

    definitions
}

fn ini_line_setting_key(
    line: &str,
    byte_start: usize,
    line_number: usize,
) -> Option<(String, SyntaxRange)> {
    let line_without_break = line.trim_end_matches(['\r', '\n']);
    let trimmed = line_without_break.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with(';')
        || trimmed.starts_with('[')
    {
        return None;
    }

    let delimiter = trimmed.find(':')?;
    if trimmed.find('=').is_some_and(|equal| equal < delimiter) {
        return None;
    }
    let raw_key = &trimmed[..delimiter];
    let key_text = raw_key.trim();
    let key = unquote_config_key(key_text)?;
    let leading_width = line_without_break.len() - line_without_break.trim_start().len();
    let key_padding = raw_key.len() - raw_key.trim_start().len();
    let key_start = byte_start + leading_width + key_padding;
    let key_end = key_start + key_text.len();

    Some((
        key,
        SyntaxRange {
            byte_start: key_start,
            byte_end: key_end,
            line_start: line_number,
            line_end: line_number,
        },
    ))
}

fn nearest_ini_section(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "section" {
            return ini_section_name(content, parent);
        }
        current = parent.parent();
    }

    None
}

fn ini_section_name(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    let section = (0..node.named_child_count()).find_map(|index| {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == "section_name").then_some(child)
    })?;
    let name = node_text(content, section);
    let name = name.trim().trim_start_matches('[').trim_end_matches(']');
    let name = unquote_config_key(name)?;
    let mut range = syntax_range(section);
    range.line_end = range.line_start;

    Some((name, range))
}

fn child_text(content: &str, node: Node<'_>, kind: &str) -> Option<String> {
    (0..node.named_child_count()).find_map(|index| {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == kind).then(|| node_text(content, child))
    })
}

fn toml_pair_key_text(content: &str, node: Node<'_>) -> Option<String> {
    toml_key_parts(content, first_key_node(node)?).map(|parts| parts.join("."))
}

fn first_key_node(node: Node<'_>) -> Option<Node<'_>> {
    (0..node.named_child_count()).find_map(|index| {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        toml_key_node(child.kind()).then_some(child)
    })
}

fn toml_key_parts(content: &str, node: Node<'_>) -> Option<Vec<String>> {
    if matches!(node.kind(), "bare_key" | "quoted_key") {
        return unquote_config_key(&node_text(content, node)).map(|key| vec![key]);
    }
    if node.kind() != "dotted_key" {
        return None;
    }
    let mut parts = Vec::new();
    for index in 0..node.named_child_count() {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        if !toml_key_node(child.kind()) {
            continue;
        }
        parts.extend(toml_key_parts(content, child)?);
    }

    (!parts.is_empty()).then_some(parts)
}

fn toml_key_node(kind: &str) -> bool {
    matches!(kind, "bare_key" | "dotted_key" | "quoted_key")
}

fn mark_prior_toml_array_table_parts(content: &str, node: Node<'_>, parts: &mut [String]) {
    if parts.len() < 2 {
        return;
    }
    let mut missing_prefix_lengths = (1..parts.len()).collect::<Vec<_>>();
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        mark_matching_toml_array_table_prefixes(
            content,
            sibling,
            parts,
            &mut missing_prefix_lengths,
        );
        if missing_prefix_lengths.is_empty() {
            return;
        }
        previous = sibling.prev_named_sibling();
    }
}

fn mark_matching_toml_array_table_prefixes(
    content: &str,
    node: Node<'_>,
    parts: &mut [String],
    missing_prefix_lengths: &mut Vec<usize>,
) {
    if missing_prefix_lengths.is_empty() {
        return;
    }
    if node.kind() == "table_array_element" {
        if let Some(key_node) = first_key_node(node) {
            if let Some(prefix) = toml_key_parts(content, key_node) {
                mark_matching_toml_array_table_prefix(parts, &prefix, missing_prefix_lengths);
            }
        }
    }
    for index in 0..node.named_child_count() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let Some(child) = node.named_child(index) else {
            continue;
        };
        mark_matching_toml_array_table_prefixes(content, child, parts, missing_prefix_lengths);
    }
}

fn mark_matching_toml_array_table_prefix(
    parts: &mut [String],
    prefix: &[String],
    missing_prefix_lengths: &mut Vec<usize>,
) {
    if prefix.is_empty() || prefix.len() >= parts.len() || !toml_parts_start_with(parts, prefix) {
        return;
    }
    let Some(index) = missing_prefix_lengths
        .iter()
        .position(|length| *length == prefix.len())
    else {
        return;
    };
    mark_toml_array_table_part(parts, prefix.len());
    missing_prefix_lengths.swap_remove(index);
}

fn toml_parts_start_with(parts: &[String], prefix: &[String]) -> bool {
    if prefix.len() > parts.len() {
        return false;
    }
    prefix
        .iter()
        .enumerate()
        .all(|(index, expected)| toml_part_name(&parts[index]) == expected)
}

fn mark_toml_array_table_part(parts: &mut [String], prefix_len: usize) {
    let Some(part) = prefix_len
        .checked_sub(1)
        .and_then(|index| parts.get_mut(index))
    else {
        return;
    };
    if !part.ends_with("[]") {
        part.push_str("[]");
    }
}

fn toml_part_name(part: &str) -> &str {
    part.strip_suffix("[]").unwrap_or(part)
}

fn sequence_suffix(path: &[Node<'_>]) -> String {
    "[]".repeat(
        path.iter()
            .filter(|node| sequence_node(node.kind()))
            .count(),
    )
}

fn sequence_node(kind: &str) -> bool {
    matches!(kind, "array" | "block_sequence" | "flow_sequence")
}

fn ancestor_pair_keys(
    content: &str,
    node: Node<'_>,
    pair_kinds: &[&str],
    key_fn: fn(&str, Node<'_>) -> Option<String>,
    suffix_fn: fn(&[Node<'_>]) -> String,
) -> Vec<String> {
    let mut keys = Vec::new();
    let mut path = vec![node];
    let mut current = node.parent();
    while let Some(parent) = current {
        if pair_kinds.contains(&parent.kind()) {
            if let Some(key) = key_fn(content, parent) {
                keys.push(format!("{}{}", key, suffix_fn(&path)));
            }
        }
        path.push(parent);
        current = parent.parent();
    }
    keys.reverse();
    keys
}

fn json_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key = node.child_by_field_name("key")?;
    unquote_config_key(&node_text(content, key))
}

fn yaml_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key = node.child_by_field_name("key")?;
    let text = scalar_text(content, key);
    unquote_config_key(&text)
}

fn scalar_text(content: &str, node: Node<'_>) -> String {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "string_scalar"
                | "plain_scalar"
                | "single_quote_scalar"
                | "double_quote_scalar"
                | "boolean_scalar"
                | "integer_scalar"
                | "float_scalar"
                | "null_scalar"
        ) {
            return node_text(content, current);
        }
        for index in (0..current.child_count()).rev() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            if let Some(child) = current.child(index) {
                stack.push(child);
            }
        }
    }
    node_text(content, node)
}

fn properties_key(content: &str, node: Node<'_>) -> Option<String> {
    (0..node.named_child_count()).find_map(|index| {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == "key").then(|| node_text(content, child))
    })
}

fn unquote_config_key(value: &str) -> Option<String> {
    let key = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned();
    (!key.is_empty() && !key.contains('\n')).then_some(key)
}
