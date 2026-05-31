use super::super::super::nodes::SyntaxRange;

pub(in crate::code::parser) fn manual_file_definitions(
    content: &str,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let lines = source_lines(content);
    let mut definitions = Vec::new();
    let mut index = 0usize;
    let mut pending_header = None::<String>;
    while index < lines.len() {
        let code = line_code_without_comment(&lines[index].code)
            .trim()
            .to_owned();
        if let Some(header) = pending_header.as_mut() {
            if !code.is_empty() {
                header.push(' ');
                header.push_str(&code);
            }
            if cpp_class_header_opens_body(header) {
                pending_header = None;
                index = collect_class_member_declarations(&lines, index + 1, &mut definitions);
            } else {
                if code.ends_with(';') {
                    pending_header = None;
                }
                index += 1;
            }
        } else if cpp_class_header_opens_body(&code) {
            index = collect_class_member_declarations(&lines, index + 1, &mut definitions);
        } else if cpp_class_header_starts(&code) && !code.ends_with(';') {
            pending_header = Some(code);
            index += 1;
        } else {
            index += 1;
        }
    }

    definitions
}

#[derive(Clone)]
struct SourceLine {
    number: usize,
    byte_start: usize,
    byte_end: usize,
    code: String,
}

struct PendingDeclaration {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
    line_end: usize,
    text: String,
}

fn source_lines(content: &str) -> Vec<SourceLine> {
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

fn cpp_class_header_opens_body(line: &str) -> bool {
    let code = line.trim_start();
    code.contains('{') && cpp_class_header_starts(code)
}

fn cpp_class_header_starts(code: &str) -> bool {
    let code = code.trim_start();
    code.starts_with("class ")
        || code
            .strip_prefix("struct ")
            .is_some_and(cpp_struct_header_has_inheritance)
}

fn cpp_struct_header_has_inheritance(header: &str) -> bool {
    header.contains(": public")
        || header.contains(": protected")
        || header.contains(": private")
        || header.contains(": virtual")
}

fn collect_class_member_declarations(
    lines: &[SourceLine],
    mut index: usize,
    definitions: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> usize {
    let mut depth = 1isize;
    let mut pending = None;
    let mut doc_start = None;

    while index < lines.len() && depth > 0 {
        let line = &lines[index];
        let code = line_code_without_comment(&line.code);
        let trimmed_code = code.trim();
        if depth == 1 {
            if trimmed_code.starts_with("};") {
                return index + 1;
            }
            if class_member_line_opens_nested_body(trimmed_code) {
                pending = None;
                doc_start = None;
            } else {
                collect_top_level_member_line(line, &mut pending, &mut doc_start, definitions);
            }
        }
        depth += brace_delta(code);
        index += 1;
    }

    index
}

fn collect_top_level_member_line(
    line: &SourceLine,
    pending: &mut Option<PendingDeclaration>,
    doc_start: &mut Option<(usize, usize)>,
    definitions: &mut Vec<(String, &'static str, SyntaxRange)>,
) {
    let trimmed = line.code.trim();
    let is_preprocessor_directive = cpp_preprocessor_directive(trimmed);
    if trimmed.is_empty() || cpp_access_label(trimmed) || is_preprocessor_directive {
        if pending.is_none() && !is_preprocessor_directive {
            *doc_start = None;
        }
        return;
    }
    if trimmed.starts_with("//") {
        doc_start.get_or_insert((line.byte_start, line.number));
        return;
    }

    let declaration = pending.get_or_insert_with(|| {
        let (byte_start, line_start) = doc_start.take().unwrap_or((line.byte_start, line.number));
        PendingDeclaration {
            byte_start,
            byte_end: line.byte_end,
            line_start,
            line_end: line.number,
            text: String::new(),
        }
    });
    if !declaration.text.is_empty() {
        declaration.text.push('\n');
    }
    declaration.text.push_str(&line.code);

    let code = line_code_without_comment(&line.code);
    if !trailing_annotation_line(code.trim()) {
        declaration.byte_end = line.byte_end;
        declaration.line_end = line.number;
    }
    if !line_code_without_comment(&line.code)
        .trim_end()
        .ends_with(';')
    {
        return;
    }
    let declaration = pending.take().expect("pending declaration should exist");
    if let Some(name) = member_function_declaration_name(&declaration.text) {
        definitions.push((
            name,
            "function_declaration",
            SyntaxRange {
                byte_start: declaration.byte_start,
                byte_end: declaration.byte_end,
                line_start: declaration.line_start,
                line_end: declaration.line_end,
            },
        ));
    }
}

fn line_code_without_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

fn line_without_block_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut code = String::new();
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        if *in_block_comment {
            let Some(comment_end) = rest.find("*/") else {
                break;
            };
            index += comment_end + "*/".len();
            *in_block_comment = false;
        } else if rest.starts_with("//") {
            code.push_str(rest);
            break;
        } else if rest.starts_with("/*") {
            index += "/*".len();
            *in_block_comment = true;
        } else {
            let character = rest
                .chars()
                .next()
                .expect("non-empty rest should yield a character");
            code.push(character);
            index += character.len_utf8();
        }
    }

    code
}

fn class_member_line_opens_nested_body(trimmed_code: &str) -> bool {
    !trimmed_code.starts_with("//") && trimmed_code.contains('{')
}

fn trailing_annotation_line(code: &str) -> bool {
    let Some((name, rest)) = code.trim_end_matches(';').trim().split_once('(') else {
        return false;
    };
    let name = name.trim();
    !name.is_empty()
        && rest.trim_end().ends_with(')')
        && name
            .chars()
            .all(|character| character == '_' || character.is_ascii_uppercase())
}

fn cpp_access_label(trimmed: &str) -> bool {
    matches!(trimmed, "public:" | "private:" | "protected:")
}

fn cpp_preprocessor_directive(trimmed: &str) -> bool {
    trimmed.starts_with('#')
}

fn brace_delta(code: &str) -> isize {
    let mut delta = 0isize;
    for character in code.chars() {
        match character {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn member_function_declaration_name(statement: &str) -> Option<String> {
    let code = statement
        .lines()
        .map(line_code_without_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if code.contains("= delete") || code.contains("= default") || code.starts_with("using ") {
        return None;
    }
    let parameter_start = top_level_parameter_start(&code)?;
    let (name_start, name_end) = name_bounds_before_open(&code, parameter_start)?;
    if code[..name_start].trim_end().ends_with('~') {
        return None;
    }
    let name = &code[name_start..name_end];
    function_name_candidate(name).then(|| name.to_owned())
}

fn top_level_parameter_start(code: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in code.char_indices() {
        match character {
            '(' if depth == 0 && parameter_open_looks_like_function(code, index) => {
                return Some(index);
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn parameter_open_looks_like_function(code: &str, parameter_start: usize) -> bool {
    if code[parameter_start + 1..]
        .trim_start()
        .starts_with(['*', '&'])
    {
        return false;
    }
    if code[..parameter_start].trim_end().len() != parameter_start {
        return false;
    }
    name_bounds_before_open(code, parameter_start)
        .map(|(start, end)| function_name_candidate(&code[start..end]))
        .unwrap_or(false)
}

fn name_bounds_before_open(code: &str, parameter_start: usize) -> Option<(usize, usize)> {
    let name_end = code[..parameter_start].trim_end().len();
    let name_start = code[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some((name_start, name_end))
}

fn function_name_candidate(name: &str) -> bool {
    if matches!(
        name,
        "if" | "for" | "while" | "switch" | "return" | "sizeof" | "void"
    ) {
        return false;
    }
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
