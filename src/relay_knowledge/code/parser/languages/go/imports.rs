pub(in crate::code::parser) fn import_specs(import_declaration: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut search_start = 0usize;
    while let Some((quote_start, quote)) = next_import_quote(import_declaration, search_start) {
        let Some(quote_end) = import_declaration[quote_start + quote.len_utf8()..]
            .find(quote)
            .map(|offset| quote_start + quote.len_utf8() + offset)
        else {
            break;
        };
        let import_path = &import_declaration[quote_start + quote.len_utf8()..quote_end];
        if let Some(spec) = import_spec_before_quote(import_declaration, quote_start, import_path)
            && !specs.contains(&spec)
        {
            specs.push(spec);
        }
        search_start = quote_end + quote.len_utf8();
    }

    specs
}

fn next_import_quote(value: &str, start: usize) -> Option<(usize, char)> {
    let mut line_comment = false;
    let mut block_comment = false;
    let mut characters = value[start..].char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        let index = start + offset;
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if character == '*' && value[index + character.len_utf8()..].starts_with('/') {
                characters.next();
                block_comment = false;
            }
            continue;
        }
        if character == '/' && value[index + character.len_utf8()..].starts_with('/') {
            characters.next();
            line_comment = true;
            continue;
        }
        if character == '/' && value[index + character.len_utf8()..].starts_with('*') {
            characters.next();
            block_comment = true;
            continue;
        }
        if matches!(character, '"' | '`') {
            return Some((index, character));
        }
    }

    None
}

fn import_spec_before_quote(
    import_declaration: &str,
    quote_start: usize,
    import_path: &str,
) -> Option<String> {
    if import_path.trim().is_empty() {
        return None;
    }
    let prefix_start = import_declaration[..quote_start]
        .rfind(['\n', '(', ';'])
        .map_or(0, |index| index + 1);
    let raw_prefix = import_declaration[prefix_start..quote_start].trim();
    if raw_prefix.contains("//")
        || raw_prefix.starts_with("/*")
        || raw_prefix.rfind("/*") > raw_prefix.rfind("*/")
    {
        return None;
    }
    let prefix = raw_prefix
        .strip_prefix("import")
        .map_or(raw_prefix, str::trim);
    let alias = prefix
        .split_whitespace()
        .last()
        .filter(|value| matches!(*value, "." | "_") || go_identifier(value));

    Some(match alias {
        Some(alias) => format!("{alias} {import_path}"),
        None => import_path.to_owned(),
    })
}

fn go_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::import_specs;

    #[test]
    fn import_specs_ignore_quotes_inside_multiline_comments() {
        let specs = import_specs(
            r#"
import (
    "context"
    /*
       alias "example.com/commented"
       "example.com/also-commented"
    */
    named "example.com/used"
)
"#,
        );

        assert_eq!(specs, ["context", "named example.com/used"]);
    }
}
