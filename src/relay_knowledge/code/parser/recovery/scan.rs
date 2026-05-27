#[derive(Default)]
pub(super) struct CodeScanState {
    in_block_comment: bool,
    in_string: bool,
    in_char: bool,
    escaped: bool,
}

impl CodeScanState {
    pub(super) fn line_complete(&self) -> bool {
        !self.in_string && !self.in_char && !self.escaped
    }

    pub(super) fn closed(&self) -> bool {
        !self.in_block_comment && self.line_complete()
    }
}

pub(super) fn first_code_char_index(text: &str, wanted: char) -> Option<usize> {
    let mut state = CodeScanState::default();
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let line = segment
            .strip_suffix('\n')
            .unwrap_or(segment)
            .strip_suffix('\r')
            .unwrap_or(segment.strip_suffix('\n').unwrap_or(segment));
        let mut found = None;
        scan_code_line_indices_with_state(line, &mut state, |index, character| {
            if character == wanted && found.is_none() {
                found = Some(offset + index);
            }
        });
        if found.is_some() {
            return found;
        }
        if !state.line_complete() {
            return None;
        }
        offset += segment.len();
    }

    None
}

pub(super) fn line_has_balanced_delimiters(line: &str) -> bool {
    let mut parentheses = 0isize;
    let mut brackets = 0isize;
    let mut invalid_order = false;
    let literals_closed = scan_code_line(line, |character| {
        match character {
            '(' => parentheses += 1,
            ')' => parentheses -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            _ => {}
        }
        if parentheses < 0 || brackets < 0 {
            invalid_order = true;
        }
    });

    literals_closed && !invalid_order && parentheses == 0 && brackets == 0
}

pub(super) fn scan_code_line(line: &str, mut visit: impl FnMut(char)) -> bool {
    scan_code_line_indices(line, |_, character| visit(character))
}

pub(in crate::code::parser) fn scan_code_line_indices(
    line: &str,
    mut visit: impl FnMut(usize, char),
) -> bool {
    let mut state = CodeScanState::default();
    scan_code_line_indices_with_state(line, &mut state, |index, character| {
        visit(index, character);
    });
    state.closed()
}

pub(in crate::code::parser) fn token_starts_in_angle_arguments(
    head: &str,
    token_start: usize,
) -> bool {
    let mut angle_depth = 0isize;
    scan_code_line_indices(head, |index, character| {
        if index >= token_start {
            return;
        }
        match character {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            _ => {}
        }
    });
    angle_depth > 0
}

pub(in crate::code::parser) fn code_contains_char(text: &str, wanted: char) -> bool {
    let mut found = false;
    scan_code_line_indices(text, |_, character| found |= character == wanted);
    found
}

pub(in crate::code::parser) fn parameter_list_has_empty_slot(parameters: &str) -> bool {
    let mut paren_depth = 0isize;
    let mut bracket_depth = 0isize;
    let mut angle_depth = 0isize;
    let mut segment_has_code = false;
    let mut empty_slot = false;
    let literals_closed = scan_code_line_indices(parameters, |_, character| {
        if empty_slot {
            return;
        }
        match character {
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '<' if paren_depth == 0 && bracket_depth == 0 => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            ',' if paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 => {
                if !segment_has_code {
                    empty_slot = true;
                }
                segment_has_code = false;
                return;
            }
            _ => {}
        }
        if !character.is_ascii_whitespace() {
            segment_has_code = true;
        }
    });

    !literals_closed
        || empty_slot
        || (parameters.trim_end().ends_with(',') && !parameters.is_empty())
}

pub(super) fn scan_code_line_indices_with_state(
    mut line: &str,
    state: &mut CodeScanState,
    mut visit: impl FnMut(usize, char),
) {
    let mut offset = 0usize;

    while !line.is_empty() {
        let Some(character) = line.chars().next() else {
            break;
        };
        let width = character.len_utf8();
        let rest = &line[width..];

        if state.in_block_comment {
            if character == '*' && rest.starts_with('/') {
                line = &rest[1..];
                offset += width + 1;
                state.in_block_comment = false;
            } else {
                line = rest;
                offset += width;
            }
            continue;
        }
        if state.in_string {
            if state.escaped {
                state.escaped = false;
            } else if character == '\\' {
                state.escaped = true;
            } else if character == '"' {
                state.in_string = false;
            }
            line = rest;
            offset += width;
            continue;
        }
        if state.in_char {
            if state.escaped {
                state.escaped = false;
            } else if character == '\\' {
                state.escaped = true;
            } else if character == '\'' {
                state.in_char = false;
            }
            line = rest;
            offset += width;
            continue;
        }

        if character == '/' && rest.starts_with('/') {
            let Some(newline_index) = rest.find('\n') else {
                break;
            };
            line = &rest[newline_index + 1..];
            offset += width + newline_index + 1;
            continue;
        }
        if character == '/' && rest.starts_with('*') {
            line = &rest[1..];
            offset += width + 1;
            state.in_block_comment = true;
            continue;
        }
        if character == '"' {
            state.in_string = true;
        } else if character == '\'' {
            state.in_char = true;
        } else {
            visit(offset, character);
        }
        line = rest;
        offset += width;
    }
}
