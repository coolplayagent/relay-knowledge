use super::shared::javascript_regex_literal_can_start;

pub(in crate::code::parser) fn javascript_code_lines_without_comments(
    content: &str,
) -> Vec<String> {
    let mut state = JavascriptLineState::default();
    content
        .lines()
        .map(|line| javascript_code_line_without_comments(line, &mut state))
        .collect()
}

#[derive(Default)]
struct JavascriptLineState {
    in_block_comment: bool,
    quote: Option<char>,
    escaped: bool,
}

fn javascript_code_line_without_comments(line: &str, state: &mut JavascriptLineState) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut suppress_continued_quote = state.quote.is_some();
    while let Some(character) = chars.next() {
        if state.in_block_comment {
            if character == '*' && chars.peek() == Some(&'/') {
                chars.next();
                state.in_block_comment = false;
            }
            continue;
        }
        if let Some(quote_char) = state.quote {
            if suppress_continued_quote {
                if state.escaped {
                    state.escaped = false;
                    continue;
                }
                if character == '\\' {
                    state.escaped = true;
                    continue;
                }
                if character == quote_char {
                    state.quote = None;
                    suppress_continued_quote = false;
                }
                continue;
            }
            result.push(character);
            if state.escaped {
                state.escaped = false;
                continue;
            }
            if character == '\\' {
                state.escaped = true;
                continue;
            }
            if character == quote_char {
                state.quote = None;
            }
            continue;
        }
        if character == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if character == '/' && chars.peek() == Some(&'*') {
            chars.next();
            state.in_block_comment = true;
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            state.quote = Some(character);
            state.escaped = false;
        }
        result.push(character);
    }
    if state.quote.is_some_and(|quote| quote != '`') && !state.escaped {
        state.quote = None;
    }
    state.escaped = false;
    result
}

pub(in crate::code::parser) fn statement_ends_with_semicolon(segment: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut last_non_space = None;
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
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            continue;
        }
        if !character.is_whitespace() {
            last_non_space = Some(character);
        }
    }
    last_non_space == Some(';')
}

pub(in crate::code::parser) fn find_javascript_pattern_outside_strings(
    line: &str,
    pattern: &str,
) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut regex_literal = false;
    let mut regex_class = false;
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
        if regex_literal {
            if escaped {
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '[' => regex_class = true,
                ']' if regex_class => regex_class = false,
                '/' if !regex_class => regex_literal = false,
                _ => {}
            }
            continue;
        }
        if line[index..].starts_with(pattern) {
            return Some(index);
        }
        match character {
            '\'' | '"' | '`' => {
                quote = Some(character);
                escaped = false;
            }
            '/' if javascript_regex_literal_can_start(&line[..index]) => {
                regex_literal = true;
                regex_class = false;
                escaped = false;
            }
            _ => {}
        }
    }
    None
}
