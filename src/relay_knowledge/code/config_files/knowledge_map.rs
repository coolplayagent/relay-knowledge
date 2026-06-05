use super::{
    model::ConfigFact,
    source::{push_definition, source_lines},
};
use crate::project::KNOWLEDGE_MAP_RELATIVE_PATH;

pub(super) fn facts(
    path: &str,
    language_id: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
) {
    if path != KNOWLEDGE_MAP_RELATIVE_PATH || language_id != "yaml" {
        return;
    }

    let mut in_topics = false;
    let mut topic_list_indent = None;
    let mut topic_item_indent = None;
    for line in source_lines(content) {
        let code = yaml_code_prefix(line.text);
        let trimmed = code.trim();
        if let Some(section) = top_level_yaml_section(code) {
            in_topics = section == "topics";
            topic_list_indent = None;
            topic_item_indent = None;
            continue;
        }
        if !in_topics || trimmed.is_empty() {
            continue;
        }

        let indent = leading_spaces(code);
        if let Some(item) = trimmed.strip_prefix("- ") {
            if !accept_topic_item_indent(&mut topic_list_indent, indent) {
                continue;
            }
            topic_item_indent = Some(indent);
            let item = item.trim_start();
            if let Some(id) = item.strip_prefix("id:") {
                push_topic_definition(definitions, id, line.range());
            }
            continue;
        }
        if trimmed == "-" {
            if !accept_topic_item_indent(&mut topic_list_indent, indent) {
                continue;
            }
            topic_item_indent = Some(indent);
            continue;
        }
        if topic_item_indent.is_some_and(|item_indent| indent == item_indent + 2) {
            let Some(id) = trimmed.strip_prefix("id:") else {
                continue;
            };
            push_topic_definition(definitions, id, line.range());
        }
    }
}

fn push_topic_definition(
    definitions: &mut Vec<ConfigFact>,
    value: &str,
    range: super::model::ConfigRange,
) {
    let name = value.trim().trim_matches('"').trim_matches('\'');
    push_definition(definitions, name, "knowledge_map_topic", range);
}

fn accept_topic_item_indent(topic_list_indent: &mut Option<usize>, indent: usize) -> bool {
    match *topic_list_indent {
        Some(list_indent) => indent == list_indent,
        None => {
            *topic_list_indent = Some(indent);
            true
        }
    }
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn yaml_code_prefix(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            '\\' if in_double && !escaped => escaped = true,
            '"' if !in_single && !escaped => in_double = !in_double,
            '\'' if !in_double => in_single = !in_single,
            '#' if !in_single && !in_double => return &line[..index],
            _ => escaped = false,
        }
        if character != '\\' {
            escaped = false;
        }
    }

    line
}

fn top_level_yaml_section(line: &str) -> Option<&str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let key = line.trim().strip_suffix(':')?;
    if key.is_empty() || key.contains(' ') {
        return None;
    }

    Some(key)
}
