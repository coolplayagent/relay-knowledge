pub(super) fn template_interpolation_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut index = 0usize;
    let mut in_template = false;
    let mut escaped = false;
    while index < line.len() {
        let rest = &line[index..];
        let Some(character) = rest.chars().next() else {
            break;
        };
        if !in_template {
            if character == '`' {
                output.push(' ');
                in_template = true;
            } else {
                output.push(character);
            }
            index = index.saturating_add(character.len_utf8());
            continue;
        }

        if escaped {
            output.push(' ');
            escaped = false;
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '\\' {
            output.push(' ');
            escaped = true;
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '`' {
            output.push(' ');
            in_template = false;
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if rest.starts_with("${") {
            let (interpolation, consumed) = template_interpolation_body(rest);
            output.push_str(&interpolation);
            index = index.saturating_add(consumed);
            continue;
        }
        output.push(' ');
        index = index.saturating_add(character.len_utf8());
    }

    output
}

fn template_interpolation_body(value: &str) -> (String, usize) {
    let mut body = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < value.len() {
        let rest = &value[index..];
        let Some(character) = rest.chars().next() else {
            break;
        };
        if let Some(quote_character) = quote {
            body.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if rest.starts_with("//") {
            body.push_str(&" ".repeat(rest.len()));
            return (body, value.len());
        }
        if rest.starts_with("/*") {
            let consumed = rest
                .find("*/")
                .map_or(rest.len(), |end| end.saturating_add(2));
            body.push_str(&" ".repeat(consumed));
            index = index.saturating_add(consumed);
            continue;
        }
        if character == '`' {
            let (template_code, consumed) = nested_template_literal_code(rest);
            body.push_str(&template_code);
            index = index.saturating_add(consumed);
            continue;
        }
        if is_string_quote_character(character) {
            quote = Some(character);
            body.push(character);
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if rest.starts_with("${") {
            depth = depth.saturating_add(1);
            body.push_str("  ");
            index = index.saturating_add(2);
            continue;
        }
        if character == '{' {
            depth = depth.saturating_add(1);
            body.push(character);
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return (body, index.saturating_add(character.len_utf8()));
            }
            body.push(character);
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        body.push(character);
        index = index.saturating_add(character.len_utf8());
    }

    (body, index)
}

fn nested_template_literal_code(value: &str) -> (String, usize) {
    let mut body = String::new();
    let mut escaped = false;
    let mut index = 0usize;
    while index < value.len() {
        let rest = &value[index..];
        let Some(character) = rest.chars().next() else {
            break;
        };
        if escaped {
            body.push(' ');
            escaped = false;
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '\\' {
            body.push(' ');
            escaped = true;
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '`' {
            body.push(' ');
            index = index.saturating_add(character.len_utf8());
            if index > 1 {
                return (body, index);
            }
            continue;
        }
        if rest.starts_with("${") {
            let (interpolation, consumed) = template_interpolation_body(rest);
            body.push_str(&interpolation);
            index = index.saturating_add(consumed);
            continue;
        }
        body.push(' ');
        index = index.saturating_add(character.len_utf8());
    }

    (body, index)
}

fn is_string_quote_character(character: char) -> bool {
    matches!(character, '"' | '\'')
}
