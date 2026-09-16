//! Decode only statically delimited strings with proven language escapes.
pub(super) fn decode(text: &str, language: &str) -> Option<String> {
    if text.len() > 4096 {
        return None;
    }
    if language == "rust" && text.starts_with('r') {
        let quote = text.find('"')?;
        let hashes = &text[1..quote];
        if !hashes.chars().all(|c| c == '#') {
            return None;
        }
        return text[quote + 1..]
            .strip_suffix(&format!("\"{hashes}"))
            .map(str::to_owned);
    }
    let raw_python = matches!(language, "python" | "starlark")
        && matches!(text.as_bytes().first(), Some(b'r' | b'R'));
    let text = if raw_python { &text[1..] } else { text };
    let quote = text.chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') {
        return None;
    }
    if quote == '\''
        && matches!(
            language,
            "c" | "cpp" | "rust" | "csharp" | "swift" | "kotlin" | "scala"
        )
    {
        return None;
    }
    let triple = matches!(language, "python" | "starlark" | "kotlin" | "scala")
        && text.starts_with(&quote.to_string().repeat(3));
    let delimiter = quote.to_string().repeat(if triple { 3 } else { 1 });
    let inner = text.strip_prefix(&delimiter)?.strip_suffix(&delimiter)?;
    if (matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
        && quote == '`'
        && inner.contains("${"))
        || (matches!(language, "kotlin" | "scala") && inner.contains('$'))
        || (language == "php" && quote == '"' && inner.contains('$'))
        || (language == "ruby" && quote == '"' && inner.contains("#{"))
        || (language == "swift" && inner.contains("\\("))
    {
        return None;
    }
    if raw_python
        || (triple && matches!(language, "kotlin" | "scala"))
        || (language == "go" && quote == '`')
    {
        return Some(if language == "go" {
            inner.replace('\r', "")
        } else {
            inner.to_owned()
        });
    }
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            output.push(c);
            continue;
        }
        let next = chars.next()?;
        if quote == '\'' && matches!(language, "php" | "ruby") && !matches!(next, '\'' | '\\') {
            output.push('\\');
            output.push(next);
            continue;
        }
        output.push(match next {
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            // Other escape families differ between grammars; retain unknown.
            _ => return None,
        });
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_escape_and_interpolation_boundaries() {
        assert_eq!(decode("'a\\nb'", "php").as_deref(), Some("a\\nb"));
        assert_eq!(decode("'a\\nb'", "python").as_deref(), Some("a\nb"));
        assert_eq!(decode("r#\"a\\nb\"#", "rust").as_deref(), Some("a\\nb"));
        assert_eq!(decode("`a\rb`", "go").as_deref(), Some("ab"));
        assert_eq!(decode("\"${key}\"", "kotlin"), None);
        assert_eq!(decode("'a'", "c"), None);
        assert_eq!(decode("`a${b}`", "typescript"), None);
        assert_eq!(decode("r'FEATURE'", "python").as_deref(), Some("FEATURE"));
    }
}
