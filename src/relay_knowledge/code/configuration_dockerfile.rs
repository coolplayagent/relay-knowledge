use super::{ConfigRange, source_lines};

pub(super) struct LogicalLine {
    pub(super) text: String,
    pub(super) range: ConfigRange,
}

pub(super) fn logical_lines(content: &str) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut text = String::new();
    let mut range = None;
    for line in source_lines(content) {
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
                range: range
                    .take()
                    .expect("Dockerfile logical line should have range"),
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

pub(super) fn from_image<'a>(words: &[&'a str]) -> Option<&'a str> {
    let image = words
        .iter()
        .skip(1)
        .copied()
        .find(|word| !word.starts_with("--"))?;
    (!image.contains('$')).then_some(image)
}

pub(super) fn stage_name<'a>(words: &[&'a str]) -> Option<&'a str> {
    let stage_index = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("AS"))?;
    words.get(stage_index + 1).copied()
}

pub(super) fn copy_from_sources<'a>(words: &[&'a str]) -> Vec<&'a str> {
    if !words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("COPY"))
    {
        return Vec::new();
    }

    let mut sources = Vec::new();
    let mut index = 1usize;
    while let Some(word) = words.get(index).copied() {
        if let Some(source) = word.strip_prefix("--from=") {
            if !source.contains('$') {
                sources.push(source);
            }
        } else if word == "--from"
            && let Some(source) = words.get(index + 1).copied()
        {
            if !source.contains('$') {
                sources.push(source);
            }
            index += 1;
        }
        index += 1;
    }

    sources
}
