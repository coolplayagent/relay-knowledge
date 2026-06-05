use super::super::{
    model::{ConfigFact, ConfigImport, ConfigRange, ConfigReference},
    source::{
        assignment, push_definition, push_import, push_reference, source_lines, strip_line_comment,
        valid_config_character, valid_config_key, valid_target,
    },
};

pub(in crate::code::config_files) fn facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    for line in logical_lines(content) {
        let trimmed = line.text.trim();
        if let Some(rule) = trimmed.strip_prefix("rule ").map(str::trim) {
            push_definition(definitions, rule, "rule", line.range);
        }
        if let Some(rest) = trimmed.strip_prefix("build ")
            && let Some((outputs, inputs)) = rest.split_once(':')
        {
            for output in outputs.split_whitespace().filter(|part| valid_target(part)) {
                push_definition(definitions, output, "target", line.range);
            }
            for input in inputs
                .split_whitespace()
                .skip(1)
                .filter(|part| valid_target(part))
            {
                push_reference(references, input, "target", line.range);
            }
        }
        if let Some((name, value)) = assignment(trimmed) {
            push_definition(definitions, name, "variable", line.range);
            for reference in variables(value) {
                push_reference(references, reference, "variable", line.range);
            }
        }
    }
}

pub(in crate::code::config_files) fn imports(content: &str, imports: &mut Vec<ConfigImport>) {
    for line in source_lines(content) {
        let trimmed = strip_line_comment(line.text).trim();
        for prefix in ["include ", "subninja "] {
            if let Some(module) = trimmed
                .strip_prefix(prefix)
                .map(str::trim)
                .and_then(static_include_module)
            {
                push_import(imports, module, line.range());
            }
        }
    }
}

struct LogicalLine {
    text: String,
    range: ConfigRange,
}

fn logical_lines(content: &str) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut text = String::new();
    let mut range = None;

    for line in source_lines(content) {
        let raw = strip_line_comment(line.text).trim_end();
        let continued = raw.ends_with('$');
        let segment = raw
            .strip_suffix('$')
            .map(str::trim_end)
            .unwrap_or(raw)
            .trim();

        let current = range.get_or_insert_with(|| line.range());
        current.byte_end = line.byte_end;
        current.line_end = line.number;
        if !text.is_empty() && !segment.is_empty() {
            text.push(' ');
        }
        text.push_str(segment);

        if continued {
            continue;
        }
        if !text.is_empty() {
            lines.push(LogicalLine {
                text: std::mem::take(&mut text),
                range: range.take().expect("logical line range should exist"),
            });
        } else {
            range = None;
        }
    }

    if !text.is_empty()
        && let Some(range) = range
    {
        lines.push(LogicalLine { text, range });
    }

    lines
}

fn variables(line: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(index) = line[offset..].find('$') {
        let start = offset + index;
        let rest = &line[start + '$'.len_utf8()..];
        if rest.starts_with('$') {
            offset = start + 2 * '$'.len_utf8();
            continue;
        }
        if let Some(braced) = rest.strip_prefix('{') {
            if let Some(end) = braced.find('}') {
                let value = &braced[..end];
                if valid_config_key(value) {
                    values.push(value);
                }
                offset = start + "${".len() + end + '}'.len_utf8();
                continue;
            }
        }

        let end = rest
            .find(|character: char| !valid_config_character(character))
            .unwrap_or(rest.len());
        let value = &rest[..end];
        if valid_config_key(value) {
            values.push(value);
        }
        offset = start + '$'.len_utf8() + end.max(1);
    }

    values
}

fn static_include_module(module: &str) -> Option<&str> {
    (!module.contains('$')).then_some(module)
}
