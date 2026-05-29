#[derive(Default)]
pub(super) struct BlockScalarTracker {
    parent_indent: Option<usize>,
}

impl BlockScalarTracker {
    pub(super) fn should_skip(&mut self, line: &str, trimmed: &str) -> bool {
        if trimmed.is_empty() {
            return self.parent_indent.is_some();
        }

        let indent = line.len() - line.trim_start().len();
        if let Some(parent_indent) = self.parent_indent {
            if indent > parent_indent {
                return true;
            }
            self.parent_indent = None;
        }

        if block_scalar_starts(trimmed) {
            self.parent_indent = Some(indent);
        }
        false
    }
}

fn block_scalar_starts(trimmed: &str) -> bool {
    let marker = if let Some((_, value)) = trimmed.split_once(':') {
        value
    } else {
        trimmed.strip_prefix('-').map(str::trim_start).unwrap_or("")
    };
    let marker = marker.split('#').next().unwrap_or(marker).trim();
    let Some(first) = marker.chars().next() else {
        return false;
    };
    matches!(first, '|' | '>')
        && marker[1..]
            .chars()
            .all(|character| matches!(character, '+' | '-') || character.is_ascii_digit())
}

pub(super) fn mapping_key(trimmed: &str) -> Option<&str> {
    let candidate = trimmed
        .strip_prefix('-')
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let separator = mapping_separator(candidate)?;

    Some(&candidate[..separator])
}

fn mapping_separator(value: &str) -> Option<usize> {
    let mut index = 0;
    let mut quote = None;
    let mut escaped = false;
    while index < value.len() {
        let rest = &value[index..];
        let character = rest
            .chars()
            .next()
            .expect("index should point to a character boundary");
        if escaped {
            escaped = false;
            index += character.len_utf8();
            continue;
        }
        if quote == Some('"') && character == '\\' {
            escaped = true;
            index += character.len_utf8();
            continue;
        }
        if quote == Some('\'') && rest.starts_with("''") {
            index += "''".len();
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            index += character.len_utf8();
            continue;
        }
        if character == ':' && quote.is_none() {
            let after = &value[index + ':'.len_utf8()..];
            if after.chars().next().is_none_or(char::is_whitespace) {
                return Some(index);
            }
        }
        index += character.len_utf8();
    }

    None
}
