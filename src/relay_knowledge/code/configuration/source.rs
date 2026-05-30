use super::model::{ConfigFact, ConfigImport, ConfigRange, ConfigReference};

pub(super) fn source_lines(content: &str) -> Vec<ConfigLine<'_>> {
    let mut lines = Vec::new();
    let mut byte_start = 0usize;
    for (index, raw_line) in content.split_inclusive('\n').enumerate() {
        let without_lf = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let text = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        lines.push(ConfigLine {
            number: index + 1,
            byte_start,
            byte_end: byte_start + text.len(),
            text,
        });
        byte_start += raw_line.len();
    }
    if content.is_empty() {
        lines.push(ConfigLine {
            number: 1,
            byte_start: 0,
            byte_end: 0,
            text: "",
        });
    }

    lines
}

pub(super) struct ConfigLine<'a> {
    pub(super) number: usize,
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) text: &'a str,
}

impl ConfigLine<'_> {
    pub(super) fn range(&self) -> ConfigRange {
        ConfigRange {
            byte_start: self.byte_start,
            byte_end: self.byte_end,
            line_start: self.number,
            line_end: self.number,
        }
    }
}

pub(super) fn push_definition(
    definitions: &mut Vec<ConfigFact>,
    name: impl AsRef<str>,
    kind: &'static str,
    range: ConfigRange,
) {
    let name = clean_name(name.as_ref());
    if !name.is_empty()
        && !definitions.iter().any(|existing| {
            existing.name == name && existing.kind == kind && existing.range == range
        })
    {
        definitions.push(ConfigFact { name, kind, range });
    }
}

pub(super) fn push_reference(
    references: &mut Vec<ConfigReference>,
    name: impl AsRef<str>,
    kind: &'static str,
    range: ConfigRange,
) {
    let name = clean_name(name.as_ref());
    if !name.is_empty()
        && !references.iter().any(|existing| {
            existing.name == name && existing.kind == kind && existing.range == range
        })
    {
        references.push(ConfigReference { name, kind, range });
    }
}

pub(super) fn push_import(
    imports: &mut Vec<ConfigImport>,
    module: impl AsRef<str>,
    range: ConfigRange,
) {
    let module = clean_name(module.as_ref());
    if !module.is_empty()
        && !imports
            .iter()
            .any(|existing| existing.module == module && existing.range == range)
    {
        imports.push(ConfigImport { module, range });
    }
}

fn clean_name(value: &str) -> String {
    unquote(value)
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(')')
        .trim_end_matches('}')
        .to_owned()
}

pub(super) fn unquote(value: &str) -> &str {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
}

pub(super) fn config_key_prefix(value: &str) -> &str {
    value
        .trim()
        .trim_start_matches('-')
        .trim()
        .trim_start_matches('{')
        .trim_start_matches('[')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
}

pub(super) fn first_quoted(value: &str) -> Option<&str> {
    let quote = value
        .chars()
        .find(|character| matches!(character, '"' | '\''))?;
    let start = value.find(quote)? + quote.len_utf8();
    let rest = &value[start..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

pub(super) fn call_name(value: &str) -> Option<&str> {
    value
        .split('(')
        .next()
        .map(str::trim)
        .filter(|name| valid_config_key(name))
}

pub(super) fn call_args_prefix<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    if !line
        .get(..command.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(command))
    {
        return None;
    }
    let rest = line.get(command.len()..)?.trim_start();
    rest.strip_prefix('(')
}

pub(super) fn paren_delta(value: &str) -> i32 {
    value.chars().fold(0, |balance, character| {
        balance
            + match character {
                '(' => 1,
                ')' => -1,
                _ => 0,
            }
    })
}

pub(super) fn assignment(line: &str) -> Option<(&str, &str)> {
    for marker in ["?=", "+=", ":=", "="] {
        if let Some((left, right)) = line.split_once(marker) {
            let name = left.trim();
            if valid_config_key(name) {
                return Some((name, right.trim()));
            }
        }
    }

    None
}

pub(super) fn variable_references<'a>(line: &'a str, prefix: &str, suffix: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    for part in line.split(prefix).skip(1) {
        let value = if suffix == " " {
            part.split(|character: char| !valid_config_character(character))
                .next()
        } else {
            part.split(suffix).next()
        };
        if let Some(value) = value.filter(|value| valid_config_key(value)) {
            values.push(value);
        }
    }

    values
}

pub(super) fn template_variables(line: &str) -> Vec<&str> {
    line.split("{{")
        .skip(1)
        .filter_map(|part| part.split("}}").next())
        .map(str::trim)
        .filter_map(|part| part.split(['|', '.', ' ', '(']).next())
        .map(str::trim)
        .filter(|name| valid_config_key(name))
        .collect()
}

pub(super) fn identifier_prefix(value: &str) -> Option<&str> {
    value
        .split(|character: char| !valid_config_character(character))
        .next()
        .filter(|name| valid_config_key(name))
}

pub(super) fn valid_target(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('.')
        && value
            .chars()
            .all(|character| valid_config_character(character) || matches!(character, '/' | ':'))
}

pub(super) fn valid_config_key(value: &str) -> bool {
    !value.is_empty() && value.chars().all(valid_config_character)
}

pub(super) fn valid_config_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | ':')
}

pub(super) fn strip_line_comment(value: &str) -> &str {
    value.split('#').next().unwrap_or(value)
}

pub(super) fn strip_inline_hash_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == '#' && quote.is_none() {
            return &value[..index];
        }
    }

    value
}
