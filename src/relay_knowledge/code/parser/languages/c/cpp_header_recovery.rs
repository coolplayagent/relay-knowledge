use super::super::super::nodes::SyntaxRange;

pub(in crate::code::parser) fn manual_file_definitions(
    content: &str,
) -> Vec<(String, Option<String>, &'static str, SyntaxRange)> {
    let lines = source_lines(content);
    let mut definitions = Vec::new();
    let mut index = 0usize;
    let mut pending_header = None::<String>;
    while index < lines.len() {
        let line_code = line_code_without_comment(&lines[index].code);
        let code = line_code.trim();
        if let Some(header) = pending_header.as_mut() {
            if !code.is_empty() {
                header.push(' ');
                header.push_str(code);
            }
            if cpp_class_header_opens_body(header) {
                let owner = cpp_class_header_name(header);
                pending_header = None;
                let body_start = top_level_body_open_start(line_code).unwrap_or(line_code.len());
                let body_end = collect_class_member_declarations(
                    &lines,
                    index,
                    body_start + "{".len(),
                    owner,
                    &mut definitions,
                );
                index = body_end.index + 1;
            } else {
                if code.ends_with(';') {
                    pending_header = None;
                }
                index += 1;
            }
        } else if cpp_class_header_opens_body(line_code) {
            let body_start = top_level_body_open_start(line_code).unwrap_or(line_code.len());
            let body_end = collect_class_member_declarations(
                &lines,
                index,
                body_start + "{".len(),
                cpp_class_header_name(line_code),
                &mut definitions,
            );
            index = body_end.index + 1;
        } else if cpp_class_header_starts(code) && !code.ends_with(';') {
            pending_header = Some(code.to_owned());
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

struct BodyEnd {
    index: usize,
    line_code_start: usize,
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
    top_level_body_open_start(line).is_some_and(|body_start| {
        let header = &line[..body_start];
        cpp_class_header_starts(header)
    })
}

fn cpp_class_header_starts(code: &str) -> bool {
    cpp_class_header_name(code).is_some()
}

fn cpp_class_header_name(header: &str) -> Option<String> {
    let search_end = top_level_body_open_start(header).unwrap_or(header.len());
    let header = &header[..search_end];
    let identifiers = identifier_spans(header);
    let mut name = None;
    for (position, (start, end)) in identifiers.iter().copied().enumerate() {
        let token = &header[start..end];
        if !matches!(token, "class" | "struct") {
            continue;
        }
        if position > 0 {
            let (previous_start, previous_end) = identifiers[position - 1];
            if &header[previous_start..previous_end] == "enum" {
                continue;
            }
        }
        if let Some(candidate) = class_declarator_name(&header[end..]) {
            name = Some(candidate);
        }
    }
    name
}

fn class_declarator_name(declarator: &str) -> Option<String> {
    let declarator_end = top_level_class_declarator_end(declarator);
    identifier_spans_outside_groups(&declarator[..declarator_end])
        .into_iter()
        .filter_map(|(start, end)| {
            let candidate = &declarator[start..end];
            (function_name_candidate(candidate) && !class_declarator_noise_token(candidate))
                .then(|| candidate.to_owned())
        })
        .next_back()
}

fn top_level_class_declarator_end(declarator: &str) -> usize {
    top_level_character_start(declarator, ':').unwrap_or(declarator.len())
}

fn class_declarator_noise_token(token: &str) -> bool {
    matches!(
        token,
        "class"
            | "struct"
            | "typename"
            | "final"
            | "sealed"
            | "abstract"
            | "alignas"
            | "__attribute__"
            | "attribute"
            | "__declspec"
            | "__declspec__"
    ) || (token.contains('_') && uppercase_decorator_name(token))
}

fn collect_class_member_declarations(
    lines: &[SourceLine],
    mut index: usize,
    mut line_code_start: usize,
    owner: Option<String>,
    definitions: &mut Vec<(String, Option<String>, &'static str, SyntaxRange)>,
) -> BodyEnd {
    let mut pending = None;
    let mut doc_start = None;

    while index < lines.len() {
        let line = &lines[index];
        let code = line_code_without_comment(&line.code);
        let code_start = skip_body_close_terminator(code, line_code_start.min(code.len()));
        line_code_start = 0;
        let code = &code[code_start..];
        let Some((delimiter_start, delimiter)) = first_top_level_body_delimiter(code) else {
            let member_line = source_line_fragment(line, code_start, line.code.len());
            collect_top_level_member_line(
                &member_line,
                owner.as_deref(),
                &mut pending,
                &mut doc_start,
                definitions,
            );
            index += 1;
            continue;
        };

        let member_line = source_line_fragment(line, code_start, code_start + delimiter_start);
        match delimiter {
            '}' => {
                collect_top_level_member_line(
                    &member_line,
                    owner.as_deref(),
                    &mut pending,
                    &mut doc_start,
                    definitions,
                );
                return BodyEnd {
                    index,
                    line_code_start: code_start + delimiter_start + "}".len(),
                };
            }
            '{' => {
                collect_terminated_member_prefix(
                    &member_line,
                    owner.as_deref(),
                    &mut pending,
                    &mut doc_start,
                    definitions,
                );
                pending = None;
                doc_start = None;
                let nested_owner = cpp_class_header_name(&member_line.code)
                    .map(|name| combine_owner(owner.as_deref(), &name));
                let body_start = code_start + delimiter_start + "{".len();
                let body_end = if let Some(nested_owner) = nested_owner {
                    collect_class_member_declarations(
                        lines,
                        index,
                        body_start,
                        Some(nested_owner),
                        definitions,
                    )
                } else {
                    skip_nested_body(lines, index, body_start)
                };
                index = body_end.index;
                line_code_start = body_end.line_code_start;
            }
            _ => unreachable!("body delimiter should be an opening or closing brace"),
        }
    }

    BodyEnd {
        index,
        line_code_start: 0,
    }
}

fn source_line_fragment(line: &SourceLine, code_start: usize, code_end: usize) -> SourceLine {
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

fn collect_top_level_member_line(
    line: &SourceLine,
    owner: Option<&str>,
    pending: &mut Option<PendingDeclaration>,
    doc_start: &mut Option<(usize, usize)>,
    definitions: &mut Vec<(String, Option<String>, &'static str, SyntaxRange)>,
) {
    collect_member_fragments(line, owner, pending, doc_start, definitions, true);
}

fn collect_terminated_member_prefix(
    line: &SourceLine,
    owner: Option<&str>,
    pending: &mut Option<PendingDeclaration>,
    doc_start: &mut Option<(usize, usize)>,
    definitions: &mut Vec<(String, Option<String>, &'static str, SyntaxRange)>,
) {
    collect_member_fragments(line, owner, pending, doc_start, definitions, false);
}

fn collect_member_fragments(
    line: &SourceLine,
    owner: Option<&str>,
    pending: &mut Option<PendingDeclaration>,
    doc_start: &mut Option<(usize, usize)>,
    definitions: &mut Vec<(String, Option<String>, &'static str, SyntaxRange)>,
    include_unterminated_tail: bool,
) {
    let visible = line.code.trim();
    if visible.starts_with("//") {
        doc_start.get_or_insert((line.byte_start, line.number));
        return;
    }
    let code = line_code_without_comment(&line.code);
    let trimmed = code.trim();
    let is_preprocessor_directive = cpp_preprocessor_directive(trimmed);
    if trimmed.is_empty() || cpp_access_label(trimmed) || is_preprocessor_directive {
        if pending.is_none() && !is_preprocessor_directive {
            *doc_start = None;
        }
        return;
    }

    let mut fragment_start = 0usize;
    let mut saw_terminated_fragment = false;
    for semicolon in top_level_semicolon_positions(code) {
        saw_terminated_fragment = true;
        let fragment = source_line_fragment(line, fragment_start, semicolon + ";".len());
        collect_member_fragment(&fragment, owner, pending, doc_start, definitions);
        fragment_start = semicolon + ";".len();
    }

    if include_unterminated_tail {
        let tail = source_line_fragment(line, fragment_start, code.len());
        collect_member_fragment(&tail, owner, pending, doc_start, definitions);
    } else if !saw_terminated_fragment && pending.is_none() {
        *doc_start = None;
    }
}

fn collect_member_fragment(
    line: &SourceLine,
    owner: Option<&str>,
    pending: &mut Option<PendingDeclaration>,
    doc_start: &mut Option<(usize, usize)>,
    definitions: &mut Vec<(String, Option<String>, &'static str, SyntaxRange)>,
) {
    let trimmed = line.code.trim();
    let is_preprocessor_directive = cpp_preprocessor_directive(trimmed);
    if trimmed.is_empty() || cpp_access_label(trimmed) || is_preprocessor_directive {
        if pending.is_none() && !is_preprocessor_directive {
            *doc_start = None;
        }
        return;
    }
    let declaration = pending.get_or_insert_with(|| {
        let (byte_start, line_start) = doc_start.take().unwrap_or_else(|| {
            (
                line.byte_start + member_declaration_start_offset(&line.code),
                line.number,
            )
        });
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
        let qualified_name = owner.map(|owner| format!("{owner}.{name}"));
        definitions.push((
            name,
            qualified_name,
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

fn member_declaration_start_offset(code: &str) -> usize {
    let mut offset = first_non_whitespace_offset(code);
    let trimmed = &code[offset..];
    for label in ["public:", "private:", "protected:"] {
        if let Some(rest) = trimmed.strip_prefix(label) {
            offset += label.len() + leading_whitespace_len(rest);
            break;
        }
    }
    if code[offset..].starts_with(';') {
        let rest = &code[offset + ";".len()..];
        offset += ";".len() + leading_whitespace_len(rest);
    }
    offset
}

fn first_non_whitespace_offset(code: &str) -> usize {
    code.char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map_or(0, |(index, _)| index)
}

fn leading_whitespace_len(code: &str) -> usize {
    code.len() - code.trim_start().len()
}

fn line_code_without_comment(line: &str) -> &str {
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

fn next_character(value: &str) -> char {
    value
        .chars()
        .next()
        .expect("non-empty value should yield a character")
}

fn skip_body_close_terminator(code: &str, start: usize) -> usize {
    let start = start.min(code.len());
    let rest = &code[start..];
    let whitespace = rest.len() - rest.trim_start().len();
    let after_whitespace = start + whitespace;
    if code[after_whitespace..].starts_with(';') {
        let after_semicolon = after_whitespace + ";".len();
        let tail = &code[after_semicolon..];
        return after_semicolon + tail.len() - tail.trim_start().len();
    }
    start
}

fn combine_owner(owner: Option<&str>, name: &str) -> String {
    owner.map_or_else(|| name.to_owned(), |owner| format!("{owner}.{name}"))
}

fn skip_nested_body(lines: &[SourceLine], mut index: usize, mut line_code_start: usize) -> BodyEnd {
    let mut depth = 1usize;
    while index < lines.len() {
        let line = &lines[index];
        let code = line_code_without_comment(&line.code);
        let mut code_start = line_code_start.min(code.len());
        line_code_start = 0;
        while code_start < code.len() {
            let Some((delimiter_start, delimiter)) =
                first_top_level_body_delimiter(&code[code_start..])
            else {
                break;
            };
            code_start += delimiter_start + delimiter.len_utf8();
            match delimiter {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return BodyEnd {
                            index,
                            line_code_start: code_start,
                        };
                    }
                }
                _ => unreachable!("body delimiter should be an opening or closing brace"),
            }
        }
        index += 1;
    }

    BodyEnd {
        index,
        line_code_start: 0,
    }
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

fn top_level_body_open_start(code: &str) -> Option<usize> {
    top_level_body_delimiter_start(code, '{')
}

fn top_level_body_delimiter_start(code: &str, delimiter: char) -> Option<usize> {
    first_top_level_body_delimiter(code)
        .and_then(|(index, character)| (character == delimiter).then_some(index))
}

fn first_top_level_body_delimiter(code: &str) -> Option<(usize, char)> {
    top_level_character_start_where(code, |character| matches!(character, '{' | '}'))
}

fn top_level_character_start(code: &str, target: char) -> Option<usize> {
    top_level_character_start_where(code, |character| character == target).map(|(index, _)| index)
}

fn top_level_semicolon_positions(code: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut search_start = 0usize;
    while search_start < code.len() {
        let Some((position, _)) =
            top_level_character_start_where(&code[search_start..], |character| character == ';')
        else {
            break;
        };
        let absolute = search_start + position;
        positions.push(absolute);
        search_start = absolute + ";".len();
    }
    positions
}

fn top_level_character_start_where(
    code: &str,
    mut predicate: impl FnMut(char) -> bool,
) -> Option<(usize, char)> {
    let mut index = 0usize;
    let mut string_delimiter = None;
    let mut escaped = false;
    let mut parameter_depth = 0usize;
    let mut bracket_depth = 0usize;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        if let Some(delimiter) = string_delimiter {
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => string_delimiter = Some(character),
            '(' => parameter_depth += 1,
            ')' => parameter_depth = parameter_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            character if parameter_depth == 0 && bracket_depth == 0 && predicate(character) => {
                return Some((index, character));
            }
            _ => {}
        }
        index += character.len_utf8();
    }

    None
}

fn identifier_spans(code: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        if !identifier_start(character) {
            index += character.len_utf8();
            continue;
        }
        let start = index;
        index += character.len_utf8();
        while index < code.len() {
            let rest = &code[index..];
            let character = next_character(rest);
            if !identifier_continue(character) {
                break;
            }
            index += character.len_utf8();
        }
        spans.push((start, index));
    }
    spans
}

fn identifier_spans_outside_groups(code: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    let mut group_depth = 0usize;
    let mut string_delimiter = None;
    let mut escaped = false;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        if let Some(delimiter) = string_delimiter {
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => {
                string_delimiter = Some(character);
                index += character.len_utf8();
            }
            '(' | '[' | '<' => {
                group_depth += 1;
                index += character.len_utf8();
            }
            ')' | ']' | '>' => {
                group_depth = group_depth.saturating_sub(1);
                index += character.len_utf8();
            }
            _ if group_depth == 0 && identifier_start(character) => {
                let start = index;
                index += character.len_utf8();
                while index < code.len() {
                    let rest = &code[index..];
                    let character = next_character(rest);
                    if !identifier_continue(character) {
                        break;
                    }
                    index += character.len_utf8();
                }
                spans.push((start, index));
            }
            _ => {
                index += character.len_utf8();
            }
        }
    }
    spans
}

fn member_function_declaration_name(statement: &str) -> Option<String> {
    let code = statement
        .lines()
        .map(line_code_without_comment)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if special_member_assignment(&code, "delete")
        || special_member_assignment(&code, "default")
        || code.starts_with("using ")
    {
        return None;
    }
    let parameter_start = top_level_parameter_start(&code)?;
    let (name_start, name_end) = name_bounds_before_open(&code, parameter_start)?;
    if code[..name_start].trim_end().ends_with('~')
        || contains_operator_keyword(&code[..parameter_start])
    {
        return None;
    }
    let name = &code[name_start..name_end];
    function_name_candidate(name).then(|| name.to_owned())
}

fn special_member_assignment(code: &str, keyword: &str) -> bool {
    let code = code.trim_end().trim_end_matches(';').trim_end();
    let Some(prefix) = code.strip_suffix(keyword) else {
        return false;
    };
    prefix.trim_end().ends_with('=')
}

fn contains_operator_keyword(prefix: &str) -> bool {
    prefix
        .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .any(|token| token == "operator")
}

fn top_level_parameter_start(code: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        match character {
            '(' if depth == 0 && parameter_open_looks_like_function(code, index) => {
                if let Some(after_decorator) = parameter_decorator_end(code, index) {
                    index = after_decorator;
                    continue;
                }
                return Some(index);
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += character.len_utf8();
    }

    None
}

fn parameter_decorator_end(code: &str, parameter_start: usize) -> Option<usize> {
    let (name_start, name_end) = name_bounds_before_open(code, parameter_start)?;
    let name = &code[name_start..name_end];
    if !member_decorator_name(name) {
        return None;
    }
    let group_end = matching_parameter_end(code, parameter_start)?;
    let rest = code[group_end + ")".len()..].trim_start();
    (rest.contains('(') && rest.trim_end_matches(';').chars().any(identifier_start))
        .then_some(group_end + ")".len())
}

fn member_decorator_name(name: &str) -> bool {
    matches!(
        name,
        "__attribute__"
            | "attribute"
            | "__declspec"
            | "__declspec__"
            | "__always_inline"
            | "always_inline"
    ) || uppercase_decorator_name(name)
}

fn uppercase_decorator_name(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && name.chars().all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

fn matching_parameter_end(code: &str, parameter_start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = parameter_start;
    let mut string_delimiter = None;
    let mut escaped = false;
    while index < code.len() {
        let rest = &code[index..];
        let character = next_character(rest);
        if let Some(delimiter) = string_delimiter {
            index += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                string_delimiter = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => string_delimiter = Some(character),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += character.len_utf8();
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
    characters.next().is_some_and(identifier_start) && characters.all(identifier_continue)
}

fn identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn identifier_continue(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::manual_file_definitions;

    #[test]
    fn manual_file_definitions_recover_same_line_and_exported_class_members() {
        let definitions = manual_file_definitions(
            r#"
class Compact { public: void Bar(); };
LEVELDB_EXPORT class ExportedDB {
 public:
  __attribute__((warn_unused_result)) Status Open();
};
"#,
        );

        assert!(definitions.iter().any(|(name, qualified, kind, _)| {
            name == "Bar"
                && qualified.as_deref() == Some("Compact.Bar")
                && *kind == "function_declaration"
        }));
        assert!(definitions.iter().any(|(name, qualified, kind, _)| {
            name == "Open"
                && qualified.as_deref() == Some("ExportedDB.Open")
                && *kind == "function_declaration"
        }));
    }
}
