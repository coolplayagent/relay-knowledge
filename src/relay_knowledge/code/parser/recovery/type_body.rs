use super::{
    first_code_char_index,
    scan::{CodeScanState, scan_code_line_indices_with_state},
};

pub(super) fn decorated_type_error_body_is_declaration_like(text: &str) -> bool {
    let Some(open_brace) = first_code_char_index(text, '{') else {
        return false;
    };
    let Some(close_brace) = text.rfind('}') else {
        return false;
    };
    if close_brace <= open_brace {
        return false;
    }

    let mut brace_depth = 0isize;
    let mut parentheses = 0isize;
    let mut brackets = 0isize;
    let mut scan_state = CodeScanState::default();
    for line in text[open_brace + 1..close_brace].lines() {
        let continued_before = parentheses > 0 || brackets > 0;
        let brace_depth_before = brace_depth;
        let mut code = String::new();
        let mut invalid_order = false;
        let mut empty_assignment = false;
        let mut pending_assignment = false;
        scan_code_line_indices_with_state(line, &mut scan_state, |_, character| {
            code.push(character);
            match character {
                '(' => parentheses += 1,
                ')' => parentheses -= 1,
                '[' => brackets += 1,
                ']' => brackets -= 1,
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
            if pending_assignment && !character.is_ascii_whitespace() {
                empty_assignment |= character == ';';
                pending_assignment = false;
            }
            if character == '=' {
                pending_assignment = true;
            }
            if parentheses < 0 || brackets < 0 || brace_depth < 0 {
                invalid_order = true;
            }
        });
        if invalid_order || !scan_state.line_complete() {
            return false;
        }
        let trimmed = code.trim();
        if trimmed.is_empty() || matches!(trimmed, "public:" | "protected:" | "private:") {
            continue;
        }
        if empty_assignment {
            return false;
        }
        let continued_after = parentheses > 0 || brackets > 0;
        if !continued_before
            && !continued_after
            && !decorated_type_error_body_line_is_declaration_like(trimmed, brace_depth_before)
        {
            return false;
        }
    }

    scan_state.closed() && brace_depth == 0 && parentheses == 0 && brackets == 0
}

fn decorated_type_error_body_line_is_declaration_like(line: &str, brace_depth: isize) -> bool {
    if line.contains("=;") || line.contains("= ;") {
        return false;
    }
    if brace_depth == 0 && decorated_type_error_body_top_level_statement(line) {
        return false;
    }
    if brace_depth > 0 {
        return line.ends_with(';') || line.ends_with('{') || line.ends_with('}');
    }

    line.ends_with(';') || line.ends_with('{') || line.starts_with('}')
}

fn decorated_type_error_body_top_level_statement(line: &str) -> bool {
    let first_token = line
        .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .find(|token| !token.is_empty());
    line.contains("++")
        || line.contains("--")
        || first_token.is_some_and(|token| {
            matches!(
                token,
                "break"
                    | "case"
                    | "continue"
                    | "default"
                    | "do"
                    | "else"
                    | "for"
                    | "goto"
                    | "if"
                    | "return"
                    | "switch"
                    | "while"
            )
        })
}
