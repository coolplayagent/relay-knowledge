const MAX_SPRING_MAPPING_ANNOTATION_LINES: usize = 12;

pub(super) fn spring_route_annotation_offset(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
            continue;
        }
        if character == '@'
            && spring_annotation_name_at(line, index).is_some_and(is_spring_route_annotation_name)
        {
            return Some(index);
        }
    }
    None
}

pub(super) fn spring_annotation_statement_from_offset(
    lines: &[String],
    start: usize,
    first_line_offset: usize,
) -> (String, usize) {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_open = false;
    let mut consumed = 0usize;
    for line in lines
        .iter()
        .skip(start)
        .take(MAX_SPRING_MAPPING_ANNOTATION_LINES)
    {
        let trimmed = line.trim();
        let segment = if consumed == 0 {
            trimmed.get(first_line_offset..).unwrap_or(trimmed)
        } else {
            trimmed
        };
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        consumed += 1;
        for character in segment.chars() {
            if let Some(quote_char) = quote {
                if escaped {
                    escaped = false;
                    continue;
                }
                if character == '\\' {
                    escaped = true;
                    continue;
                }
                if character == quote_char {
                    quote = None;
                }
                continue;
            }
            match character {
                '"' => quote = Some(character),
                '(' => {
                    depth += 1;
                    saw_open = true;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    if saw_open && depth == 0 {
                        return (statement, consumed);
                    }
                }
                _ => {}
            }
        }
        if !saw_open {
            return (statement, consumed);
        }
    }
    (statement, consumed.max(1))
}

pub(super) fn spring_statement_after_annotation(statement: &str) -> &str {
    let trimmed = statement.trim();
    if !trimmed.starts_with('@') {
        return "";
    }
    let after_at = &trimmed[1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    let rest = after_at[name_end..].trim_start();
    if !rest.starts_with('(') {
        return rest;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in rest.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return &rest[index + character.len_utf8()..];
                }
            }
            _ => {}
        }
    }
    ""
}

pub(super) fn spring_tail_after_leading_annotations(mut tail: &str) -> &str {
    loop {
        let trimmed = tail.trim_start();
        if !trimmed.starts_with('@') {
            return trimmed;
        }
        let next_tail = spring_statement_after_annotation(trimmed);
        if next_tail.len() == trimmed.len() {
            return trimmed;
        }
        tail = next_tail;
    }
}

fn spring_annotation_name_at(line: &str, at_index: usize) -> Option<&str> {
    let after_at = &line[at_index + 1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    let annotation_name = &after_at[..name_end];
    (!annotation_name.is_empty()).then(|| {
        annotation_name
            .rsplit('.')
            .next()
            .unwrap_or(annotation_name)
    })
}

fn is_spring_route_annotation_name(annotation: &str) -> bool {
    matches!(
        annotation,
        "GetMapping"
            | "PostMapping"
            | "PutMapping"
            | "DeleteMapping"
            | "PatchMapping"
            | "RequestMapping"
    )
}

#[cfg(test)]
#[path = "statements_tests.rs"]
mod tests;
