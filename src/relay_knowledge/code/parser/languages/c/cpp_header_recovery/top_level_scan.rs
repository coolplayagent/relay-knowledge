//! Literal- and group-aware C/C++ delimiter and identifier scanning.

use super::source_text::next_character;

pub(super) fn top_level_body_open_start(code: &str) -> Option<usize> {
    top_level_body_delimiter_start(code, '{')
}

fn top_level_body_delimiter_start(code: &str, delimiter: char) -> Option<usize> {
    first_top_level_body_delimiter(code)
        .and_then(|(index, character)| (character == delimiter).then_some(index))
}

pub(super) fn first_top_level_body_delimiter(code: &str) -> Option<(usize, char)> {
    top_level_character_start_where(code, |character| matches!(character, '{' | '}'))
}

pub(super) fn top_level_character_start(code: &str, target: char) -> Option<usize> {
    top_level_character_start_where(code, |character| character == target).map(|(index, _)| index)
}

pub(super) fn top_level_semicolon_positions(code: &str) -> Vec<usize> {
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

pub(super) fn identifier_spans(code: &str) -> Vec<(usize, usize)> {
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

pub(super) fn identifier_spans_outside_groups(code: &str) -> Vec<(usize, usize)> {
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

pub(super) fn identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

pub(super) fn identifier_continue(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[cfg(test)]
#[path = "top_level_scan_tests.rs"]
mod tests;
