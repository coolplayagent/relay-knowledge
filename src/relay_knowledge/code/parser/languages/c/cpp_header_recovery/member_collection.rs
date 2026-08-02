//! C++ class-member collection, nested-body traversal, and source-range materialization.

use super::{
    declarators::{cpp_class_header_name, member_function_declaration_name},
    source_text::{SourceLine, line_code_without_comment, source_line_fragment},
    top_level_scan::{first_top_level_body_delimiter, top_level_semicolon_positions},
};
use crate::code::parser::nodes::SyntaxRange;

struct PendingDeclaration {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
    line_end: usize,
    text: String,
}

pub(super) struct BodyEnd {
    pub(super) index: usize,
    line_code_start: usize,
}

pub(super) fn collect_class_member_declarations(
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
    if !code.trim_end().ends_with(';') {
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

#[cfg(test)]
#[path = "member_collection_tests.rs"]
mod tests;
