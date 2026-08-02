mod declarators;
mod member_collection;
mod source_text;
mod top_level_scan;

use super::super::super::nodes::SyntaxRange;
use declarators::{cpp_class_header_name, cpp_class_header_opens_body, cpp_class_header_starts};
use member_collection::collect_class_member_declarations;
use source_text::{line_code_without_comment, source_lines};
use top_level_scan::top_level_body_open_start;

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
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
