use super::{ConfigRange, source_lines, strip_line_comment, valid_config_key};

pub(super) struct LogicalLine {
    pub(super) text: String,
    pub(super) range: ConfigRange,
}

pub(super) fn logical_lines(content: &str) -> Vec<LogicalLine> {
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

pub(super) fn variables(line: &str) -> Vec<&str> {
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
            .find(|character: char| !super::valid_config_character(character))
            .unwrap_or(rest.len());
        let value = &rest[..end];
        if valid_config_key(value) {
            values.push(value);
        }
        offset = start + '$'.len_utf8() + end.max(1);
    }

    values
}

pub(super) fn static_include_module(module: &str) -> Option<&str> {
    (!module.contains('$')).then_some(module)
}
