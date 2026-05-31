mod node_kinds;

use super::super::nodes::SyntaxRange;

pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};

pub(in crate::code::parser) fn manual_type_references(
    content: &str,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut references = Vec::new();
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
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) {
    let Some(mut cursor) = resume_pending_signature_scan(
        line,
        line_byte_offset,
        line_number,
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
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> Option<usize> {
    match pending_signature_scan.take() {
        Some(PendingSignatureScan::ParameterAnnotation { bracket_depth }) => {
            let scan =
                scan_top_level_expression(line, bracket_depth, parameter_annotation_delimiter);
            collect_type_names(&line[..scan.end], line_byte_offset, line_number, references);
            Some(finish_parameter_annotation_scan(
                line,
                scan,
                pending_signature_scan,
            )?)
        }
        Some(PendingSignatureScan::ReturnAnnotation { bracket_depth }) => {
            let scan = scan_top_level_expression(line, bracket_depth, return_annotation_delimiter);
            collect_type_names(&line[..scan.end], line_byte_offset, line_number, references);
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
    pending_signature_scan: &mut Option<PendingSignatureScan>,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) -> usize {
    let scan = scan_top_level_expression(&line[annotation_start..], 0, return_annotation_delimiter);
    let annotation_end = annotation_start + scan.end;
    collect_type_names(
        &line[annotation_start..annotation_end],
        line_byte_offset + annotation_start,
        line_number,
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

fn collect_type_names(
    annotation: &str,
    annotation_byte_offset: usize,
    line_number: usize,
    references: &mut Vec<(String, &'static str, SyntaxRange)>,
) {
    let bytes = annotation.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
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
        if type_reference_name(name) {
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
mod tests {
    use super::manual_type_references;

    fn reference_names(content: &str) -> Vec<String> {
        manual_type_references(content)
            .into_iter()
            .map(|(name, _, _)| name)
            .collect()
    }

    #[test]
    fn keeps_commas_inside_generic_type_annotations() {
        let names = reference_names(
            "def save(request: dict[str, W3ConnectorSaveRequest]) -> Result[ConnectorItem, SaveError]:\n    pass\n",
        );

        assert!(names.iter().any(|name| name == "W3ConnectorSaveRequest"));
        assert!(names.iter().any(|name| name == "ConnectorItem"));
        assert!(names.iter().any(|name| name == "SaveError"));
    }

    #[test]
    fn ignores_colons_inside_default_expressions() {
        let names = reference_names(
            "def save(request: W3ConnectorSaveRequest = {'fallback': Bar}) -> None:\n    pass\n",
        );

        assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
    }

    #[test]
    fn skips_unannotated_default_expression_colons() {
        let names = reference_names(
            "def save(options={fallback: Bar}, request: W3ConnectorSaveRequest = None) -> None:\n    pass\n",
        );

        assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
    }

    #[test]
    fn preserves_hash_characters_inside_string_defaults() {
        let names = reference_names(
            "def save(request: W3ConnectorSaveRequest = \"#\") -> SaveResult:\n    body: BodyType = BodyType()\n",
        );

        assert_eq!(names, vec!["W3ConnectorSaveRequest", "SaveResult"]);
    }

    #[test]
    fn carries_annotations_across_wrapped_lines() {
        let names = reference_names(
            "def save(\n    request: dict[\n        str, W3ConnectorSaveRequest\n    ],\n) -> None:\n    pass\n",
        );

        assert_eq!(names, vec!["W3ConnectorSaveRequest"]);
    }
}
