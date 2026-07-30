//! Python function-signature annotation reference scanning.

use std::collections::BTreeSet;

use crate::code::parser::nodes::SyntaxRange;

pub(in crate::code::parser) fn manual_type_references(
    content: &str,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut references = Vec::new();
    let local_type_parameters = file_local_type_parameter_names(content);
    let mut byte_offset = 0usize;
    let mut in_signature = false;
    let mut paren_depth = 0isize;
    let mut pending_signature_scan = None;

    for (line_index, line) in content.split_inclusive('\n').enumerate() {
        let code = line_without_comment(line).trim_end_matches(['\r', '\n']);
        let trimmed = code.trim_start();
        if starts_python_function_signature(trimmed) {
            in_signature = true;
            paren_depth = 0;
            pending_signature_scan = None;
        }
        if in_signature {
            collect_annotation_references(
                code,
                byte_offset,
                line_index + 1,
                &local_type_parameters,
                &mut pending_signature_scan,
                &mut references,
            );
            paren_depth += signature_paren_delta(code);
            if paren_depth <= 0 && trimmed.ends_with(':') {
                in_signature = false;
                pending_signature_scan = None;
            }
        }
        byte_offset += line.len();
    }

    references
}

fn starts_python_function_signature(trimmed: &str) -> bool {
    trimmed.starts_with("def ") || trimmed.starts_with("async def ")
}

fn signature_paren_delta(line: &str) -> isize {
    let bytes = line.as_bytes();
    let mut cursor = 0usize;
    let mut depth = 0isize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' => cursor += quoted_literal_len(&bytes[cursor..]),
            b'(' | b'[' => {
                depth += 1;
                cursor += 1;
            }
            b')' | b']' => {
                depth -= 1;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    depth
}

fn line_without_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' => cursor += quoted_literal_len(&bytes[cursor..]),
            b'#' => return &line[..cursor],
            _ => cursor += 1,
        }
    }
    line
}

#[derive(Clone, Copy)]
enum PendingSignatureScan {
    ParameterAnnotation { bracket_depth: usize },
    ReturnAnnotation { bracket_depth: usize },
    DefaultExpression { bracket_depth: usize },
}

struct ExpressionScan {
    end: usize,
    bracket_depth: usize,
    hit_delimiter: bool,
}

fn collect_annotation_references(
    line: &str,
    line_byte_offset: usize,
    line_number: usize,
    local_type_parameters: &BTreeSet<String>,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) {
    let Some(mut cursor) = resume_pending_signature_scan(
        line,
        line_byte_offset,
        line_number,
        local_type_parameters,
        pending_signature_scan,
        references,
    ) else {
        return;
    };

    let bytes = line.as_bytes();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' => cursor += quoted_literal_len(&bytes[cursor..]),
            b'-' if bytes.get(cursor + 1) == Some(&b'>') => {
                cursor = collect_return_annotation(
                    line,
                    cursor + "->".len(),
                    line_byte_offset,
                    line_number,
                    local_type_parameters,
                    pending_signature_scan,
                    references,
                );
            }
            b'=' => {
                cursor = skip_default_expression(line, cursor + 1, 0, pending_signature_scan);
            }
            b':' if annotation_name_before_colon(&line[..cursor]) => {
                cursor = collect_parameter_annotation(
                    line,
                    cursor + 1,
                    line_byte_offset,
                    line_number,
                    local_type_parameters,
                    pending_signature_scan,
                    references,
                );
            }
            _ => cursor += 1,
        }

        if pending_signature_scan.is_some() {
            return;
        }
    }
}

fn annotation_name_before_colon(prefix: &str) -> bool {
    prefix
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn resume_pending_signature_scan(
    line: &str,
    line_byte_offset: usize,
    line_number: usize,
    local_type_parameters: &BTreeSet<String>,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> Option<usize> {
    match pending_signature_scan.take() {
        Some(PendingSignatureScan::ParameterAnnotation { bracket_depth }) => {
            let scan =
                scan_top_level_expression(line, bracket_depth, parameter_annotation_delimiter);
            collect_type_names(
                &line[..scan.end],
                line_byte_offset,
                line_number,
                local_type_parameters,
                references,
            );
            Some(finish_parameter_annotation_scan(
                line,
                scan,
                pending_signature_scan,
            )?)
        }
        Some(PendingSignatureScan::ReturnAnnotation { bracket_depth }) => {
            let scan = scan_top_level_expression(line, bracket_depth, return_annotation_delimiter);
            collect_type_names(
                &line[..scan.end],
                line_byte_offset,
                line_number,
                local_type_parameters,
                references,
            );
            finish_return_annotation_scan(line, scan, pending_signature_scan)
        }
        Some(PendingSignatureScan::DefaultExpression { bracket_depth }) => {
            let scan = scan_top_level_expression(line, bracket_depth, default_expression_delimiter);
            finish_default_expression_scan(line, 0, scan, pending_signature_scan)
        }
        None => Some(0),
    }
}

fn collect_parameter_annotation(
    line: &str,
    annotation_start: usize,
    line_byte_offset: usize,
    line_number: usize,
    local_type_parameters: &BTreeSet<String>,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> usize {
    let scan =
        scan_top_level_expression(&line[annotation_start..], 0, parameter_annotation_delimiter);
    let annotation_end = annotation_start + scan.end;
    collect_type_names(
        &line[annotation_start..annotation_end],
        line_byte_offset + annotation_start,
        line_number,
        local_type_parameters,
        references,
    );
    finish_parameter_annotation_scan(&line[annotation_start..], scan, pending_signature_scan)
        .map(|cursor| annotation_start + cursor)
        .unwrap_or(line.len())
}

fn collect_return_annotation(
    line: &str,
    annotation_start: usize,
    line_byte_offset: usize,
    line_number: usize,
    local_type_parameters: &BTreeSet<String>,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> usize {
    let scan = scan_top_level_expression(&line[annotation_start..], 0, return_annotation_delimiter);
    let annotation_end = annotation_start + scan.end;
    collect_type_names(
        &line[annotation_start..annotation_end],
        line_byte_offset + annotation_start,
        line_number,
        local_type_parameters,
        references,
    );
    finish_return_annotation_scan(&line[annotation_start..], scan, pending_signature_scan)
        .map(|cursor| annotation_start + cursor)
        .unwrap_or(line.len())
}

fn finish_parameter_annotation_scan(
    line: &str,
    scan: ExpressionScan,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
) -> Option<usize> {
    if !scan.hit_delimiter {
        if scan.bracket_depth > 0 {
            *pending_signature_scan = Some(PendingSignatureScan::ParameterAnnotation {
                bracket_depth: scan.bracket_depth,
            });
        }
        return None;
    }

    if line.as_bytes().get(scan.end) == Some(&b'=') {
        return finish_default_expression_scan(
            line,
            scan.end + 1,
            scan_top_level_expression(&line[scan.end + 1..], 0, default_expression_delimiter),
            pending_signature_scan,
        );
    }

    Some(scan.end.saturating_add(1).min(line.len()))
}

fn finish_return_annotation_scan(
    line: &str,
    scan: ExpressionScan,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
) -> Option<usize> {
    if !scan.hit_delimiter {
        if scan.bracket_depth > 0 {
            *pending_signature_scan = Some(PendingSignatureScan::ReturnAnnotation {
                bracket_depth: scan.bracket_depth,
            });
        }
        return None;
    }

    Some(scan.end.saturating_add(1).min(line.len()))
}

fn skip_default_expression(
    line: &str,
    default_start: usize,
    initial_bracket_depth: usize,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
) -> usize {
    finish_default_expression_scan(
        line,
        default_start,
        scan_top_level_expression(
            &line[default_start..],
            initial_bracket_depth,
            default_expression_delimiter,
        ),
        pending_signature_scan,
    )
    .unwrap_or(line.len())
}

fn finish_default_expression_scan(
    line: &str,
    default_start: usize,
    scan: ExpressionScan,
    pending_signature_scan: &mut Option<PendingSignatureScan>,
) -> Option<usize> {
    if !scan.hit_delimiter {
        if scan.bracket_depth > 0 {
            *pending_signature_scan = Some(PendingSignatureScan::DefaultExpression {
                bracket_depth: scan.bracket_depth,
            });
        }
        return None;
    }

    Some((default_start + scan.end).saturating_add(1).min(line.len()))
}

fn parameter_annotation_delimiter(byte: u8) -> bool {
    matches!(byte, b',' | b')' | b'=')
}

fn return_annotation_delimiter(byte: u8) -> bool {
    byte == b':'
}

fn default_expression_delimiter(byte: u8) -> bool {
    matches!(byte, b',' | b')')
}

fn scan_top_level_expression(
    expression: &str,
    initial_bracket_depth: usize,
    mut delimiter: impl FnMut(u8) -> bool,
) -> ExpressionScan {
    let bytes = expression.as_bytes();
    let mut cursor = 0usize;
    let mut bracket_depth = initial_bracket_depth;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == b'\'' || byte == b'"' {
            cursor += quoted_literal_len(&bytes[cursor..]);
            continue;
        }

        match byte {
            b'(' | b'[' | b'{' => bracket_depth += 1,
            b')' if bracket_depth == 0 && delimiter(byte) => {
                return ExpressionScan {
                    end: cursor,
                    bracket_depth,
                    hit_delimiter: true,
                };
            }
            b')' if bracket_depth == 0 => {}
            b')' | b']' | b'}' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if bracket_depth == 0 && delimiter(byte) => {
                return ExpressionScan {
                    end: cursor,
                    bracket_depth,
                    hit_delimiter: true,
                };
            }
            _ => {}
        }
        cursor += 1;
    }

    ExpressionScan {
        end: bytes.len(),
        bracket_depth,
        hit_delimiter: false,
    }
}

fn quoted_literal_len(bytes: &[u8]) -> usize {
    let quote = bytes[0];
    let triple_quoted = bytes.len() >= 3 && bytes[1] == quote && bytes[2] == quote;
    let mut cursor = if triple_quoted { 3 } else { 1 };

    while cursor < bytes.len() {
        if !triple_quoted && bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if triple_quoted
            && cursor + 2 < bytes.len()
            && bytes[cursor] == quote
            && bytes[cursor + 1] == quote
            && bytes[cursor + 2] == quote
        {
            return cursor + 3;
        }
        if !triple_quoted && bytes[cursor] == quote {
            return cursor + 1;
        }
        cursor += 1;
    }

    bytes.len()
}

fn file_local_type_parameter_names(content: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in content.lines() {
        collect_typevar_assignment_name(line, &mut names);
        collect_pep695_type_parameter_names(line.trim_start(), &mut names);
    }

    names
}

fn collect_typevar_assignment_name(line: &str, names: &mut BTreeSet<String>) {
    let Some((left, right)) = line_without_comment(line).split_once('=') else {
        return;
    };
    let assignment = right.trim_start();
    if !(assignment.starts_with("TypeVar(")
        || assignment.starts_with("typing.TypeVar(")
        || assignment.starts_with("TypeVarTuple(")
        || assignment.starts_with("typing.TypeVarTuple(")
        || assignment.starts_with("ParamSpec(")
        || assignment.starts_with("typing.ParamSpec("))
    {
        return;
    }
    let name = left.trim();
    if name.bytes().next().is_some_and(identifier_start) && name.bytes().all(identifier_continue) {
        names.insert(name.to_owned());
    }
}

fn collect_pep695_type_parameter_names(line: &str, names: &mut BTreeSet<String>) {
    let Some(declaration) = line
        .strip_prefix("def ")
        .or_else(|| line.strip_prefix("async def "))
        .or_else(|| line.strip_prefix("class "))
    else {
        return;
    };
    let Some(after_name) = declaration_after_name(declaration) else {
        return;
    };
    let candidate = after_name.trim_start();
    if !candidate.starts_with('[') {
        return;
    }
    let Some(type_parameter_end) = matching_type_parameter_bracket_end(candidate) else {
        return;
    };
    let parameters = &candidate[1..type_parameter_end];
    for parameter in split_type_parameter_items(parameters) {
        collect_type_parameter_name(parameter, names);
    }
}

fn split_type_parameter_items(parameters: &str) -> Vec<&str> {
    let bytes = parameters.as_bytes();
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut cursor = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' => cursor += quoted_literal_len(&bytes[cursor..]),
            b'[' | b'(' | b'{' => {
                bracket_depth += 1;
                cursor += 1;
            }
            b']' | b')' | b'}' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                cursor += 1;
            }
            b',' if bracket_depth == 0 => {
                items.push(&parameters[start..cursor]);
                cursor += 1;
                start = cursor;
            }
            _ => cursor += 1,
        }
    }
    items.push(&parameters[start..]);

    items
}

fn collect_type_parameter_name(parameter: &str, names: &mut BTreeSet<String>) {
    let parameter = parameter.trim_start().trim_start_matches('*').trim_start();
    let bytes = parameter.as_bytes();
    if !bytes.first().copied().is_some_and(identifier_start) {
        return;
    }
    let mut end = 1usize;
    while end < bytes.len() && identifier_continue(bytes[end]) {
        end += 1;
    }
    let name = &parameter[..end];
    if !python_typing_helper(name) {
        names.insert(name.to_owned());
    }
}

fn declaration_after_name(declaration: &str) -> Option<&str> {
    let bytes = declaration.as_bytes();
    if !bytes.first().copied().is_some_and(identifier_start) {
        return None;
    }

    let mut cursor = 1usize;
    while cursor < bytes.len() && identifier_continue(bytes[cursor]) {
        cursor += 1;
    }

    Some(&declaration[cursor..])
}

fn matching_type_parameter_bracket_end(candidate: &str) -> Option<usize> {
    let bytes = candidate.as_bytes();
    let mut cursor = 1usize;
    let mut bracket_depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' => cursor += quoted_literal_len(&bytes[cursor..]),
            b'[' => {
                bracket_depth += 1;
                cursor += 1;
            }
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                if bracket_depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }

    None
}

fn collect_type_names(
    annotation: &str,
    annotation_byte_offset: usize,
    line_number: usize,
    local_type_parameters: &BTreeSet<String>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) {
    let bytes = annotation.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\'' | b'"') {
            cursor += quoted_literal_len(&bytes[cursor..]);
            continue;
        }
        if !identifier_start(bytes[cursor]) {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len() && identifier_continue(bytes[cursor]) {
            cursor += 1;
        }
        let name = &annotation[start..cursor];
        if type_reference_name(name) && !local_type_parameters.contains(name) {
            references.push((
                name.to_owned(),
                "type",
                SyntaxRange {
                    byte_start: annotation_byte_offset + start,
                    byte_end: annotation_byte_offset + cursor,
                    line_start: line_number,
                    line_end: line_number,
                },
            ));
        }
    }
}

fn identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn identifier_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn type_reference_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase() && !python_typing_helper(name))
}

fn python_typing_helper(name: &str) -> bool {
    matches!(
        name,
        "Annotated"
            | "Any"
            | "Callable"
            | "ClassVar"
            | "Dict"
            | "Final"
            | "Generic"
            | "Iterable"
            | "Iterator"
            | "List"
            | "Literal"
            | "Mapping"
            | "Optional"
            | "Protocol"
            | "Sequence"
            | "Self"
            | "Set"
            | "None"
            | "Tuple"
            | "Type"
            | "TypeAlias"
            | "Union"
    )
}

#[cfg(test)]
#[path = "annotations_tests.rs"]
mod tests;
