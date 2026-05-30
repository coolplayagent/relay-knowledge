use super::super::{
    model::{ConfigFact, ConfigRange},
    source::{push_definition, source_lines},
};

pub(in crate::code::configuration) fn facts(content: &str, definitions: &mut Vec<ConfigFact>) {
    for heading in headings(content) {
        push_definition(definitions, heading.name, "heading", heading.range);
    }
}

struct Heading {
    name: String,
    range: ConfigRange,
}

fn headings(content: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut fence = None;

    for line in source_lines(content) {
        if let Some(active) = fence {
            if markdown_structural_line(line.text)
                .is_some_and(|trimmed| closes_fence(trimmed, active))
            {
                fence = None;
            }
            continue;
        }
        let Some(trimmed) = markdown_structural_line(line.text) else {
            continue;
        };
        if let Some(marker) = fence_marker(trimmed) {
            fence = Some(marker);
            continue;
        }

        let level = trimmed
            .chars()
            .take_while(|character| *character == '#')
            .count();
        if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
            headings.push(Heading {
                name: trimmed[level..].trim().to_owned(),
                range: line.range(),
            });
        }
    }

    headings
}

fn markdown_structural_line(line: &str) -> Option<&str> {
    if line.starts_with('\t') {
        return None;
    }
    let spaces = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    (spaces <= 3).then(|| &line[spaces..])
}

fn fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed
        .chars()
        .next()
        .filter(|character| matches!(*character, '`' | '~'))?;
    let count = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (count >= 3).then_some((marker, count))
}

fn closes_fence(trimmed: &str, active: (char, usize)) -> bool {
    let (marker, count) = active;
    trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count()
        >= count
}
