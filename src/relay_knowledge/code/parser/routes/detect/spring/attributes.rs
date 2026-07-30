pub(super) fn extract_annotation_string_values(line: &str) -> Vec<String> {
    let Some(paren_pos) = line.find('(') else {
        return Vec::new();
    };
    let inner_start = &line[paren_pos + 1..];
    let inner = inner_start.trim_start();
    if inner.starts_with(['"', '{']) {
        extract_java_string_values_from_attribute_value(inner)
    } else {
        find_named_java_attribute_value(inner, "value")
            .or_else(|| find_named_java_attribute_value(inner, "path"))
            .map(extract_java_string_values_from_attribute_value)
            .unwrap_or_default()
    }
}

pub(super) fn extract_spring_method_attributes(line: &str) -> Vec<String> {
    let paren_pos = match line.find('(') {
        Some(pos) => pos,
        None => return vec!["any".to_owned()],
    };
    let inner = &line[paren_pos + 1..];
    let Some(after_eq) = find_named_java_attribute_value(inner, "method") else {
        return vec!["any".to_owned()];
    };
    let raw_value = java_attribute_value_segment(after_eq);
    let mut methods = Vec::new();
    for part in raw_value
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
    {
        let part = part.trim();
        let method_part = part.strip_prefix("RequestMethod.").unwrap_or(part);
        let method = method_part
            .trim_matches(|character: char| !character.is_ascii_alphabetic())
            .to_ascii_lowercase();
        if matches!(
            method.as_str(),
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        ) {
            methods.push(method);
        }
    }
    if methods.is_empty() {
        methods.push("any".to_owned());
    }

    methods
}

pub(super) fn spring_annotation_uses_concatenated_path(line: &str) -> bool {
    let Some(paren_pos) = line.find('(') else {
        return false;
    };
    let inner = line[paren_pos + 1..].trim_start();
    let value = if inner.starts_with('"') || inner.starts_with('{') {
        Some(inner)
    } else {
        find_named_java_attribute_value(inner, "value")
            .or_else(|| find_named_java_attribute_value(inner, "path"))
    };
    value.is_some_and(|value| {
        java_attribute_value_has_top_level_concat(java_attribute_value_segment(value))
    })
}

fn find_named_java_attribute_value<'a>(inner: &'a str, name: &str) -> Option<&'a str> {
    for argument in split_java_top_level_arguments(inner) {
        let Some(after_name) = argument.trim_start().strip_prefix(name) else {
            continue;
        };
        let after_name = after_name.trim_start();
        if let Some(after_eq) = after_name.strip_prefix('=') {
            return Some(after_eq.trim_start());
        }
    }
    None
}

fn split_java_top_level_arguments(inner: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in inner.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let argument = inner[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                return arguments;
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = inner[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let argument = inner[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

fn extract_java_string_values_from_attribute_value(value: &str) -> Vec<String> {
    let segment = java_attribute_value_segment(value);
    if java_attribute_value_has_top_level_concat(segment) {
        return Vec::new();
    }
    if segment.trim_start().starts_with('{') {
        return extract_double_quoted_java_strings(segment);
    }
    extract_double_quoted_java_string(segment)
        .into_iter()
        .collect()
}

fn java_attribute_value_segment(value: &str) -> &str {
    let value = value.trim_start();
    if value.starts_with('{') {
        let mut depth = 0usize;
        for (index, character) in value.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return &value[..=index];
                    }
                }
                _ => {}
            }
        }
        return value;
    }
    let end = value.find([',', ')']).unwrap_or(value.len());
    &value[..end]
}

fn java_attribute_value_has_top_level_concat(value: &str) -> bool {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for character in value.chars() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '+' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn extract_double_quoted_java_strings(s: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = s[offset..].find('"') {
        let start = offset + relative_start;
        let mut value = String::new();
        let mut escaped = false;
        let mut closed_at = None;
        for (relative_index, character) in s[start + 1..].char_indices() {
            if escaped {
                value.push(character);
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == '"' {
                closed_at = Some(start + 1 + relative_index + character.len_utf8());
                break;
            }
            value.push(character);
        }
        let Some(next_offset) = closed_at else {
            break;
        };
        values.push(value);
        offset = next_offset;
    }
    values
}

fn extract_double_quoted_java_string(s: &str) -> Option<String> {
    if !s.starts_with('"') {
        return None;
    }
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 1usize;
    while i < chars.len() {
        match chars[i] {
            '"' => break,
            '\\' => {
                if i + 1 < chars.len() {
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    break;
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }
    Some(result)
}

#[cfg(test)]
#[path = "attributes_tests.rs"]
mod tests;
