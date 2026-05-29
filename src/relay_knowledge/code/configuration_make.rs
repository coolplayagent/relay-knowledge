use super::{ConfigRange, source_lines};

pub(super) struct LogicalLine {
    pub(super) text: String,
    pub(super) range: ConfigRange,
}

pub(super) struct RuleParts<'a> {
    pub(super) targets: &'a str,
    pub(super) deps: &'a str,
    pub(super) static_pattern: bool,
}

pub(super) fn logical_lines(content: &str) -> Vec<LogicalLine> {
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

pub(super) fn rule_parts(line: &str) -> Option<RuleParts<'_>> {
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

pub(super) fn valid_target(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '.' | '/' | ':' | '%')
        })
}

pub(super) fn valid_reference_target(value: &str, static_pattern: bool) -> bool {
    (!static_pattern || !value.contains('%')) && valid_target(value)
}

pub(super) fn skip_define_body(trimmed: &str, in_define: &mut bool) -> bool {
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

pub(super) fn static_include_module(module: &str) -> Option<&str> {
    (!module.contains('$')).then_some(module)
}

pub(super) fn include_args(line: &str) -> Option<&str> {
    for prefix in ["-include ", "sinclude ", "include "] {
        if let Some(rest) = line.strip_prefix(prefix).map(str::trim) {
            return Some(rest);
        }
    }

    None
}

pub(super) fn assignment_line(line: &str) -> &str {
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

pub(super) fn variables(line: &str) -> Vec<&str> {
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
