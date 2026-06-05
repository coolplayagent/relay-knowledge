use super::super::{
    model::{ConfigFact, ConfigImport, ConfigRange, ConfigReference},
    source::{
        assignment, push_definition, push_import, push_reference, source_lines, strip_line_comment,
    },
};

pub(in crate::code::config_files) fn facts(
    content: &str,
    definitions: &mut Vec<ConfigFact>,
    references: &mut Vec<ConfigReference>,
) {
    let mut in_define = false;
    for line in logical_lines(content) {
        if line.text.starts_with('\t') {
            continue;
        }
        let trimmed = line.text.trim();
        if skip_define_body(trimmed, &mut in_define) {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = assignment(assignment_line(trimmed)) {
            push_definition(definitions, name, "variable", line.range);
            for reference in variables(strip_line_comment(value)) {
                push_reference(references, reference, "variable", line.range);
            }
            continue;
        }
        if let Some(rule) = rule_parts(trimmed) {
            let deps = strip_line_comment(rule.deps).trim();
            if assignment(deps).is_some() {
                continue;
            }
            let mut has_target = false;
            for target in rule
                .targets
                .split_whitespace()
                .filter(|target| valid_target(target))
            {
                has_target = true;
                push_definition(definitions, target, "target", line.range);
            }
            if has_target {
                for dep in deps
                    .split_whitespace()
                    .filter(|dep| valid_reference_target(dep, rule.static_pattern))
                {
                    push_reference(references, dep, "target", line.range);
                }
                for variable in variables(deps) {
                    push_reference(references, variable, "variable", line.range);
                }
            }
        }
    }
}

pub(in crate::code::config_files) fn imports(content: &str, imports: &mut Vec<ConfigImport>) {
    let mut in_define = false;
    for line in logical_lines(content) {
        if line.text.starts_with('\t') {
            continue;
        }
        let trimmed = line.text.trim();
        if skip_define_body(trimmed, &mut in_define) {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = include_args(trimmed) else {
            continue;
        };
        for module in strip_line_comment(rest)
            .split_whitespace()
            .filter_map(static_include_module)
        {
            push_import(imports, module, line.range);
        }
    }
}

struct LogicalLine {
    text: String,
    range: ConfigRange,
}

struct RuleParts<'a> {
    targets: &'a str,
    deps: &'a str,
    static_pattern: bool,
}

fn logical_lines(content: &str) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut text = String::new();
    let mut range = None;

    for line in source_lines(content) {
        if line.text.starts_with('\t') && text.is_empty() {
            lines.push(LogicalLine {
                text: line.text.to_owned(),
                range: line.range(),
            });
            continue;
        }

        let raw = line.text.trim_end();
        let continued = raw.ends_with('\\');
        let segment = raw
            .strip_suffix('\\')
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
                range: range.take().expect("Make logical line should have range"),
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

fn rule_parts(line: &str) -> Option<RuleParts<'_>> {
    line.split_once("::")
        .or_else(|| line.split_once(':'))
        .map(|(targets, deps)| {
            let (deps, static_pattern) = static_pattern_prerequisites(deps);
            RuleParts {
                targets: targets.trim(),
                deps: deps.trim(),
                static_pattern,
            }
        })
}

fn static_pattern_prerequisites(deps: &str) -> (&str, bool) {
    let Some((target_pattern, prerequisites)) = deps.split_once(':') else {
        return (deps, false);
    };
    if target_pattern
        .split_whitespace()
        .any(|pattern| pattern.contains('%'))
    {
        (prerequisites, true)
    } else {
        (deps, false)
    }
}

fn valid_target(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '.' | '/' | ':' | '%')
        })
}

fn valid_reference_target(value: &str, static_pattern: bool) -> bool {
    (!static_pattern || !value.contains('%')) && valid_target(value)
}

fn skip_define_body(trimmed: &str, in_define: &mut bool) -> bool {
    if *in_define {
        if trimmed == "endef" {
            *in_define = false;
        }
        return true;
    }
    if trimmed.starts_with("define ") || trimmed == "define" {
        *in_define = true;
        return true;
    }

    false
}

fn static_include_module(module: &str) -> Option<&str> {
    (!module.contains('$')).then_some(module)
}

fn include_args(line: &str) -> Option<&str> {
    for prefix in ["-include ", "sinclude ", "include "] {
        if let Some(rest) = line.strip_prefix(prefix).map(str::trim) {
            return Some(rest);
        }
    }

    None
}

fn assignment_line(line: &str) -> &str {
    let mut rest = line.trim_start();
    loop {
        let Some((modifier, tail)) = rest.split_once(char::is_whitespace) else {
            return rest;
        };
        if !matches!(modifier, "export" | "override" | "private" | "unexport") {
            return rest;
        }
        rest = tail.trim_start();
    }
}

fn variables(line: &str) -> Vec<&str> {
    let mut variables = variable_references(line, "$(", ")");
    variables.extend(variable_references(line, "${", "}"));
    variables
}

fn variable_references<'a>(line: &'a str, prefix: &str, suffix: &str) -> Vec<&'a str> {
    line.split(prefix)
        .skip(1)
        .filter_map(|part| part.split(suffix).next())
        .map(|value| value.split(':').next().unwrap_or(value))
        .filter(|value| {
            !value.is_empty()
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
                })
        })
        .collect()
}
