pub(super) fn python_code_lines_without_triple_quoted_strings(content: &str) -> Vec<String> {
    let mut delimiter = None;
    content
        .lines()
        .map(|line| {
            let line = python_code_line_without_triple_quoted_strings(line, &mut delimiter);
            python_code_line_without_comment(&line)
        })
        .collect()
}

fn python_code_line_without_triple_quoted_strings(
    line: &str,
    delimiter: &mut Option<&'static str>,
) -> String {
    let mut result = String::new();
    let mut rest = line;
    loop {
        if let Some(active_delimiter) = *delimiter {
            let Some(end_pos) = rest.find(active_delimiter) else {
                return result;
            };
            rest = &rest[end_pos + active_delimiter.len()..];
            *delimiter = None;
            continue;
        }
        let Some((start_pos, next_delimiter)) = next_python_triple_quote(rest) else {
            result.push_str(rest);
            return result;
        };
        result.push_str(&rest[..start_pos]);
        rest = &rest[start_pos + next_delimiter.len()..];
        *delimiter = Some(next_delimiter);
    }
}

fn next_python_triple_quote(value: &str) -> Option<(usize, &'static str)> {
    let single = value.find("'''").map(|index| (index, "'''"));
    let double = value.find("\"\"\"").map(|index| (index, "\"\"\""));
    match (single, double) {
        (Some(single), Some(double)) => Some(if single.0 <= double.0 { single } else { double }),
        (Some(single), None) => Some(single),
        (None, Some(double)) => Some(double),
        (None, None) => None,
    }
}

fn python_code_line_without_comment(line: &str) -> String {
    let mut result = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(quote_char) = quote {
            result.push(character);
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
        if character == '#' {
            break;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
#[path = "python_lexical_tests.rs"]
mod tests;
