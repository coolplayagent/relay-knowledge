use super::super::{
    detection, key_values,
    model::{ConfigFact, ConfigImport, ConfigReference},
    source::{
        first_quoted, push_definition, push_import, push_reference, source_lines,
        valid_config_character,
    },
};
use super::templates::strip_go_comments;

pub(in crate::code::config_files) fn facts(
    path: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    if let Some(language_id) = detection::static_template_language(path) {
        key_values::facts(language_id, content, definitions);
    }
    if !syntax(content) {
        return;
    }
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_go_comments(line.text, &mut in_comment);
        for action in ["define", "block"] {
            for name in actions(&text, action).into_iter().filter_map(first_quoted) {
                push_definition(definitions, name, "template", line.range());
            }
        }
        for action in ["template", "include", "block"] {
            for name in actions(&text, action).into_iter().filter_map(first_quoted) {
                push_reference(references, name, "template", line.range());
            }
        }
    }
}

pub(in crate::code::config_files) fn imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_go_comments(line.text, &mut in_comment);
        for action in ["template", "include", "block"] {
            for name in actions(&text, action).into_iter().filter_map(first_quoted) {
                push_import(imports, name, line.range());
            }
        }
    }
}

fn syntax(content: &str) -> bool {
    content.contains("{{")
}

fn actions<'a>(line: &'a str, action: &str) -> Vec<&'a str> {
    let mut actions = Vec::new();
    for part in line.split("{{").skip(1) {
        let part = part
            .split("}}")
            .next()
            .unwrap_or(part)
            .trim_start_matches('-')
            .trim_start();
        if let Some(rest) = part.strip_prefix(action) {
            if rest
                .chars()
                .next()
                .is_none_or(|character| !valid_config_character(character))
            {
                actions.push(rest.trim_start().trim_end_matches('-').trim_end());
            }
        }
    }

    actions
}
