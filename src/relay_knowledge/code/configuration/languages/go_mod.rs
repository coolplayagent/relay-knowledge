use super::super::{
    model::{ConfigFact, ConfigReference},
    source::{push_definition, push_reference, source_lines},
};

pub(in crate::code::configuration) fn facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_require_block = false;
    let mut in_replace_block = false;
    for line in source_lines(content) {
        let trimmed = line.text.trim();
        let code = strip_comment(trimmed).trim();
        if code.starts_with("require (") {
            in_require_block = true;
            continue;
        }
        if code.starts_with("replace (") {
            in_replace_block = true;
            continue;
        }
        if in_require_block && code == ")" {
            in_require_block = false;
            continue;
        }
        if in_replace_block && code == ")" {
            in_replace_block = false;
            continue;
        }
        if let Some(module) = code.strip_prefix("module ").map(str::trim) {
            push_definition(definitions, module, "module", line.range());
        }
        if let Some(module) = code
            .strip_prefix("require ")
            .and_then(|value| value.split_whitespace().next())
        {
            push_reference(references, module, "dependency", line.range());
        }
        if in_require_block
            && !code.is_empty()
            && let Some(module) = code.split_whitespace().next()
        {
            push_reference(references, module, "dependency", line.range());
        }
        if in_replace_block && !code.is_empty() {
            for module in replace_modules(code) {
                push_reference(references, module, "dependency", line.range());
            }
        }
        if let Some(rest) = code.strip_prefix("replace ") {
            for module in replace_modules(rest) {
                push_reference(references, module, "dependency", line.range());
            }
        }
    }
}

fn strip_comment(value: &str) -> &str {
    value
        .split("//")
        .next()
        .unwrap_or(value)
        .split('#')
        .next()
        .unwrap_or(value)
}

fn replace_modules(rest: &str) -> Vec<&str> {
    rest.split("=>")
        .filter_map(|side| {
            side.split_whitespace()
                .next()
                .filter(|module| module_path(module))
        })
        .collect()
}

fn module_path(value: &str) -> bool {
    !value.starts_with('.') && !value.starts_with('/') && !value.contains('\\')
}
