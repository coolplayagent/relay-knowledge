use super::super::{
    model::{ConfigFact, ConfigImport, ConfigReference},
    source::{
        first_quoted, identifier_prefix, push_definition, push_import, push_reference,
        source_lines, template_variables, valid_config_character,
    },
};
use super::templates::{quoted_values, strip_jinja_comments};

pub(in crate::code::configuration) fn facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_jinja_comments(line.text, &mut in_comment);
        for action in ["block", "macro"] {
            for name in jinja_actions(&text, action)
                .into_iter()
                .filter_map(identifier_prefix)
            {
                push_definition(definitions, name, "template", line.range());
            }
        }
        for name in template_variables(&text) {
            push_reference(references, name, "variable", line.range());
        }
    }
}

pub(in crate::code::configuration) fn imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_comment = false;
    for line in source_lines(content) {
        let text = strip_jinja_comments(line.text, &mut in_comment);
        for action in ["include", "extends", "import", "from"] {
            for body in jinja_actions(&text, action) {
                let values = if action == "include" {
                    quoted_values(body)
                } else {
                    first_quoted(body).into_iter().collect()
                };
                for value in values {
                    push_import(imports, value, line.range());
                }
            }
        }
    }
}

fn jinja_actions<'a>(line: &'a str, action: &str) -> Vec<&'a str> {
    let mut actions = Vec::new();
    for part in line.split("{%").skip(1) {
        let part = part
            .split("%}")
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
                actions.push(rest.trim_start());
            }
        }
    }

    actions
}
