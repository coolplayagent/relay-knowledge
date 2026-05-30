use std::path::Path;

use super::super::{
    calls::starlark_calls,
    model::{ConfigFact, ConfigImport, ConfigReference},
    source::{
        call_args_prefix, call_name, first_quoted, paren_delta, push_definition, push_import,
        push_reference, source_lines, strip_inline_hash_comment, valid_config_character,
        valid_config_key, valid_target,
    },
};

pub(in crate::code::configuration) fn facts(
    path: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_load_call = false;
    let mut rule_call_balance = 0i32;
    for line in source_lines(content) {
        let trimmed = line.text.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let code = strip_inline_hash_comment(trimmed).trim_end();
        if in_load_call {
            in_load_call = !code.contains(')');
            continue;
        }
        if call_args_prefix(code, "load").is_some() {
            in_load_call = !code.contains(')');
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("def ").and_then(call_name) {
            push_definition(definitions, name, "function", line.range());
        }
        let starts_rule_call = starlark_call_start(code).is_some();
        if (rule_call_balance > 0 || starts_rule_call)
            && let Some(name) = starlark_name_argument(code)
        {
            push_definition(definitions, name, "target", line.range());
            if let Some(qualified) = qualified_target_name(path, name) {
                push_definition(definitions, qualified, "target", line.range());
            }
        }
        if rule_call_balance > 0 || starts_rule_call {
            rule_call_balance = (rule_call_balance + paren_delta(code)).max(0);
        }
        for label in quoted_labels(code)
            .into_iter()
            .filter_map(|label| target_label(path, label))
        {
            push_reference(references, label, "target", line.range());
        }
    }
}

pub(in crate::code::configuration) fn imports(content: &str, imports: &mut Vec<ConfigImport>) {
    for call in starlark_calls(content, "load") {
        let text = load_text_without_comments(&call.text);
        if let Some(value) = first_quoted(&text) {
            push_import(imports, value, call.range);
        }
    }
}

fn load_text_without_comments(text: &str) -> String {
    text.lines()
        .map(strip_inline_hash_comment)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn target_label(path: &str, label: &str) -> Option<String> {
    if label.ends_with(".bzl") {
        return None;
    }
    if pseudo_label(label) {
        return None;
    }
    if label.starts_with("//") {
        return Some(package_label(label));
    }
    if label.starts_with('@') {
        return Some(label.to_owned());
    }
    if let Some(local) = label.strip_prefix(':') {
        return valid_target(local).then(|| local_label(path, local));
    }

    Some(label.to_owned())
}

fn pseudo_label(label: &str) -> bool {
    label.starts_with("//visibility:") || label.starts_with("//conditions:")
}

fn package_label(label: &str) -> String {
    if label.contains(':') {
        return label.to_owned();
    }
    let package = label.trim_start_matches("//");
    let Some(target) = package
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
    else {
        return label.to_owned();
    };

    format!("{label}:{target}")
}

fn local_label(path: &str, target: &str) -> String {
    let package = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    format!("//{package}:{target}")
}

fn qualified_target_name(path: &str, name: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    if !matches!(file_name, "BUILD" | "BUILD.bazel") || name.starts_with("//") {
        return None;
    }

    let package = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    Some(format!("//{package}:{name}"))
}

fn quoted_labels(line: &str) -> Vec<&str> {
    line.split(['"', '\''])
        .filter(|part| part.starts_with("//") || part.starts_with(':') || part.starts_with('@'))
        .collect()
}

fn starlark_name_argument(line: &str) -> Option<&str> {
    let mut offset = 0usize;
    while let Some(index) = line[offset..].find("name") {
        let start = offset + index;
        let before = line[..start].chars().next_back();
        let rest = &line[start + "name".len()..];
        if before.is_none_or(|character| !valid_config_character(character)) {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=').and_then(first_quoted) {
                return Some(value);
            }
        }
        offset = start + "name".len();
    }

    None
}

fn starlark_call_start(line: &str) -> Option<&str> {
    let (command, _) = line.split_once('(')?;
    let command = command.trim();
    valid_config_key(command).then_some(command)
}
