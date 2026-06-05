#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::code::feature_flags) enum ParameterBodyStatus {
    Block,
    Pending,
    LineOnly,
}

pub(in crate::code::feature_flags) fn function_parameter_receivers(
    line: &str,
    language_id: &str,
) -> Vec<String> {
    if let Some(parameters) = javascript_function_parameters(line) {
        return javascript_parameter_receivers(parameters);
    }
    if let Some(parameters) = arrow_function_parameters(line) {
        return javascript_parameter_receivers(parameters);
    }
    if let Some(parameters) = non_js_function_parameters(line) {
        return non_js_parameter_receivers(parameters, language_id);
    }

    Vec::new()
}

pub(in crate::code::feature_flags) fn function_parameter_body_status(
    line: &str,
    language_id: &str,
) -> ParameterBodyStatus {
    if let Some(remainder) = javascript_function_remainder(line) {
        return body_status_after_parameter_list(remainder);
    }
    if let Some(arrow_start) = find_code_pattern(line, "=>", 0) {
        return body_status_after_parameter_list(&line[arrow_start.saturating_add(2)..]);
    }
    if matches!(
        language_id,
        "go" | "java" | "csharp" | "kotlin" | "scala" | "swift"
    ) {
        if let Some(remainder) = non_js_function_remainder(line) {
            return body_status_after_parameter_list(remainder);
        }
    }

    ParameterBodyStatus::LineOnly
}

fn javascript_function_parameters(line: &str) -> Option<&str> {
    let function_start = find_code_pattern(line, "function ", 0)?;
    let open_paren_offset = line[function_start..].find('(')?;
    let open_paren = function_start.saturating_add(open_paren_offset);
    let close_paren_offset = line[open_paren.saturating_add(1)..].find(')')?;
    let close_paren = open_paren
        .saturating_add(1)
        .saturating_add(close_paren_offset);
    Some(&line[open_paren.saturating_add(1)..close_paren])
}

fn javascript_function_remainder(line: &str) -> Option<&str> {
    let function_start = find_code_pattern(line, "function ", 0)?;
    let open_paren_offset = line[function_start..].find('(')?;
    let open_paren = function_start.saturating_add(open_paren_offset);
    let close_paren_offset = line[open_paren.saturating_add(1)..].find(')')?;
    let close_paren = open_paren
        .saturating_add(1)
        .saturating_add(close_paren_offset);
    Some(&line[close_paren.saturating_add(1)..])
}

fn arrow_function_parameters(line: &str) -> Option<&str> {
    let arrow_start = find_code_pattern(line, "=>", 0)?;
    let prefix = line[..arrow_start].trim_end();
    if prefix.ends_with(')') {
        let open_paren = prefix.rfind('(')?;
        return Some(&prefix[open_paren.saturating_add(1)..prefix.len().saturating_sub(1)]);
    }
    let parameter_start = prefix
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            (!is_source_token_character(character))
                .then_some(index.saturating_add(character.len_utf8()))
        })
        .unwrap_or(0);
    Some(&prefix[parameter_start..])
}

fn non_js_function_parameters(line: &str) -> Option<&str> {
    let open_paren = non_js_parameter_open(line)?;
    let close_paren = matching_close_paren(line, open_paren)?;
    Some(&line[open_paren.saturating_add(1)..close_paren])
}

fn non_js_function_remainder(line: &str) -> Option<&str> {
    let open_paren = non_js_parameter_open(line)?;
    let close_paren = matching_close_paren(line, open_paren)?;
    Some(&line[close_paren.saturating_add(1)..])
}

fn non_js_parameter_open(line: &str) -> Option<usize> {
    if let Some(func_start) = find_code_pattern(line, "func ", 0) {
        return line[func_start..]
            .rfind('(')
            .map(|offset| func_start.saturating_add(offset));
    }
    if line.trim_start().starts_with("if ")
        || line.trim_start().starts_with("for ")
        || line.trim_start().starts_with("while ")
        || line.trim_start().starts_with("switch ")
        || line.trim_start().starts_with("catch ")
    {
        return None;
    }
    let first_brace = line.find('{').unwrap_or(line.len());
    if first_brace == line.len() && line.contains(';') {
        return None;
    }
    line[..first_brace].rfind('(')
}

fn matching_close_paren(line: &str, open_paren: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in line[open_paren..].char_indices() {
        if character == '(' {
            depth = depth.saturating_add(1);
            continue;
        }
        if character == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(open_paren.saturating_add(offset));
            }
        }
    }

    None
}

fn javascript_parameter_receivers(parameters: &str) -> Vec<String> {
    parameters
        .split(',')
        .filter_map(first_parameter_identifier)
        .collect()
}

fn non_js_parameter_receivers(parameters: &str, language_id: &str) -> Vec<String> {
    parameters
        .split(',')
        .filter_map(|parameter| {
            if language_id == "go" {
                first_parameter_identifier(parameter)
            } else {
                last_parameter_identifier(parameter)
            }
        })
        .collect()
}

fn first_parameter_identifier(parameter: &str) -> Option<String> {
    let parameter = parameter.trim_start();
    let name = parameter
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>();
    valid_receiver_name(&name).then_some(name)
}

fn last_parameter_identifier(parameter: &str) -> Option<String> {
    parameter
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .rev()
        .find(|candidate| valid_receiver_name(candidate) && !parameter_keyword(candidate))
        .map(ToOwned::to_owned)
}

fn body_status_after_parameter_list(value: &str) -> ParameterBodyStatus {
    let body = value.trim_start();
    if body.starts_with('{') {
        ParameterBodyStatus::Block
    } else if body.is_empty() {
        ParameterBodyStatus::Pending
    } else {
        ParameterBodyStatus::LineOnly
    }
}

fn find_code_pattern(line: &str, pattern: &str, start: usize) -> Option<usize> {
    let mut search_start = start;
    while let Some(offset) = line[search_start..].find(pattern) {
        let pattern_start = search_start.saturating_add(offset);
        if token_boundary_before(line, pattern_start) {
            return Some(pattern_start);
        }
        search_start = pattern_start.saturating_add(pattern.len());
    }

    None
}

fn token_boundary_before(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_none_or(|character| !is_source_token_character(character))
}

fn is_source_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn valid_receiver_name(receiver: &str) -> bool {
    !receiver.is_empty()
        && receiver
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && receiver
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn parameter_keyword(value: &str) -> bool {
    matches!(
        value,
        "const" | "final" | "mut" | "ref" | "out" | "in" | "params" | "var" | "val"
    )
}
