//! Byte-stable C/C++ source-line views for header recovery.

#[derive(Clone)]
pub(super) struct SourceLine {
    pub(super) number: usize,
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) code: String,
}

pub(super) fn source_lines(content: &str) -> Vec<SourceLine> {
    let mut byte_start = 0usize;
    let mut in_block_comment = false;
    let mut lines = Vec::new();
    for (index, raw_line) in content.split_inclusive('\n').enumerate() {
        let text = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let code = line_without_block_comments(text, &mut in_block_comment);
        lines.push(SourceLine {
            number: index + 1,
            byte_start,
            byte_end: byte_start + text.len(),
            code,
        });
        byte_start += raw_line.len();
    }
    if content.is_empty() || content.ends_with('\n') {
        return lines;
    }
    lines
}

pub(super) fn source_line_fragment(
    line: &SourceLine,
    code_start: usize,
    code_end: usize,
) -> SourceLine {
    if code_start == 0 && code_end >= line.code.len() {
        return line.clone();
    }
    let code_start = code_start.min(line.code.len());
    let code_end = code_end.clamp(code_start, line.code.len());
    SourceLine {
        number: line.number,
        byte_start: line.byte_start + code_start,
        byte_end: line.byte_start + code_end,
        code: line.code[code_start..code_end].to_owned(),
    }
}

pub(super) fn line_code_without_comment(line: &str) -> &str {
    line_comment_start(line).map_or(line, |start| &line[..start])
}

fn line_without_block_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut code = String::new();
    let mut index = 0usize;
    let mut string_delimiter = None;
    let mut escaped = false;
    while index < line.len() {
        let rest = &line[index..];
        if *in_block_comment {
            let Some(comment_end) = rest.find("*/") else {
                push_spaces(&mut code, rest.len());
                break;
            };
            let comment_len = comment_end + "*/".len();
            push_spaces(&mut code, comment_len);
            index += comment_len;
            *in_block_comment = false;
        } else if let Some(delimiter) = string_delimiter {
            let character = next_character(rest);
            code.push(character);
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
        } else if rest.starts_with("//") {
            code.push_str(rest);
            break;
        } else if rest.starts_with("/*") {
            push_spaces(&mut code, "/*".len());
            index += "/*".len();
            *in_block_comment = true;
        } else {
            let character = next_character(rest);
            code.push(character);
            index += character.len_utf8();
            if matches!(character, '"' | '\'') {
                string_delimiter = Some(character);
            }
        }
    }

    code
}

fn push_spaces(code: &mut String, byte_len: usize) {
    code.extend(std::iter::repeat_n(' ', byte_len));
}

fn line_comment_start(line: &str) -> Option<usize> {
    let mut index = 0usize;
    let mut string_delimiter = None;
    let mut escaped = false;
    while index < line.len() {
        let rest = &line[index..];
        if let Some(delimiter) = string_delimiter {
            let character = next_character(rest);
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
        } else if rest.starts_with("//") {
            return Some(index);
        } else {
            let character = next_character(rest);
            index += character.len_utf8();
            if matches!(character, '"' | '\'') {
                string_delimiter = Some(character);
            }
        }
    }

    None
}

pub(super) fn next_character(value: &str) -> char {
    value
        .chars()
        .next()
        .expect("non-empty value should yield a character")
}

#[cfg(test)]
#[path = "source_text_tests.rs"]
mod tests;
