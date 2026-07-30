pub(super) fn java_code_lines_without_comments(content: &str) -> Vec<String> {
    let mut state = JavaCodeLineState::default();
    content
        .lines()
        .map(|line| java_code_line_without_comments(line, &mut state))
        .collect()
}

pub(super) fn line_declares_java_type(line: &str) -> bool {
    let declaration = line.split('{').next().unwrap_or(line);
    declaration.contains(" class ")
        || declaration.contains(" interface ")
        || declaration.contains(" enum ")
        || declaration.starts_with("class ")
        || declaration.starts_with("interface ")
        || declaration.starts_with("enum ")
}

pub(super) fn line_declares_nested_java_helper_type(line: &str, brace_depth: usize) -> bool {
    if brace_depth == 0 {
        return false;
    }
    let declaration = line.split('{').next().unwrap_or(line).trim();
    line_declares_java_type(declaration)
}

pub(super) fn parse_java_method_def(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let marker = trimmed.find('(')?;
    let before_paren = &trimmed[..marker];
    let name_start = before_paren
        .rfind(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .map_or(0, |pos| pos + 1);
    let name = &before_paren[name_start..];
    if name.is_empty() || name.chars().next().is_some_and(|c| !c.is_alphanumeric()) {
        return None;
    }
    Some(name.to_owned())
}

pub(super) fn update_java_brace_depth(line: &str, brace_depth: &mut usize) {
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
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
            '"' | '\'' => quote = Some(character),
            '{' => *brace_depth += 1,
            '}' => *brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
}

#[derive(Default)]
struct JavaCodeLineState {
    in_block_comment: bool,
    in_text_block: bool,
}

fn java_code_line_without_comments(line: &str, state: &mut JavaCodeLineState) -> String {
    let mut result = String::new();
    let mut chars = line.char_indices().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some((index, character)) = chars.next() {
        if state.in_block_comment {
            if character == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                state.in_block_comment = false;
            }
            continue;
        }
        if state.in_text_block {
            if line[index..].starts_with("\"\"\"") {
                chars.next();
                chars.next();
                state.in_text_block = false;
            }
            continue;
        }
        if let Some(quote_char) = quote {
            result.push(character);
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
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '*') {
            chars.next();
            state.in_block_comment = true;
            continue;
        }
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '/') {
            break;
        }
        if character == '"' && line[index..].starts_with("\"\"\"") {
            chars.next();
            chars.next();
            state.in_text_block = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
#[path = "java_tests.rs"]
mod tests;
