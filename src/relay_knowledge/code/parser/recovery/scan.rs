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
            break;
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
