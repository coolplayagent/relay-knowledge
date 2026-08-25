//! Cross-style identifier equivalence and source-safe identifier masking.

pub(super) fn identifier_terms_equivalent(candidate: &str, token: &str) -> bool {
    if candidate.eq_ignore_ascii_case(token) {
        return true;
    }
    let candidate_singular = singular_identifier_term(candidate);
    if candidate_singular
        .as_deref()
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(token))
    {
        return true;
    }
    let Some(token_singular) = singular_identifier_term(token) else {
        return false;
    };

    candidate.eq_ignore_ascii_case(&token_singular)
        || candidate_singular
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&token_singular))
}

fn singular_identifier_term(term: &str) -> Option<String> {
    if !term.is_ascii() {
        return None;
    }
    let lower = term.to_ascii_lowercase();
    if lower.len() < 4
        || !lower
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return None;
    }
    if lower == "series" || lower == "species" {
        return None;
    }
    if lower.ends_with("ies") && lower.len() > 4 {
        let mut singular = lower[..lower.len() - 3].to_owned();
        singular.push('y');
        Some(singular)
    } else if lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
        && !lower.ends_with("is")
    {
        Some(lower[..lower.len() - 1].to_owned())
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum LexicalMode {
    Code,
    BlockComment { depth: usize },
    Quoted { quote: u8, triple: bool },
    JavaScriptRegex { in_character_class: bool },
    JavaScriptTemplate,
    RustRawString { hashes: usize },
}

const JAVASCRIPT_REGEX_PREFIX_KEYWORDS: &[&[u8]] = &[
    b"case", b"delete", b"return", b"throw", b"typeof", b"void", b"yield",
];

pub(super) fn code_outside_comments_and_literals(language_id: &str, content: &str) -> String {
    let bytes = content.as_bytes();
    let mut code = vec![b' '; bytes.len()];
    let mut mode = if starts_inside_block_comment(language_id, content) {
        LexicalMode::BlockComment { depth: 1 }
    } else {
        LexicalMode::Code
    };
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            code[index] = b'\n';
        }
        match mode {
            LexicalMode::Code => {
                if let Some((end, hashes)) = rust_raw_string_start(language_id, bytes, index) {
                    mode = LexicalMode::RustRawString { hashes };
                    index = end;
                } else if slash_line_comments(language_id) && bytes[index..].starts_with(b"//") {
                    index = mask_to_line_end(bytes, &mut code, index);
                } else if block_comments(language_id) && bytes[index..].starts_with(b"/*") {
                    mode = LexicalMode::BlockComment { depth: 1 };
                    index += 2;
                } else if hash_comment_starts(language_id, bytes, index)
                    || alternate_line_comment_starts(language_id, bytes, index)
                    || language_id == "sql" && bytes[index..].starts_with(b"--")
                {
                    index = mask_to_line_end(bytes, &mut code, index);
                } else if javascript_family(language_id) && bytes[index] == b'`' {
                    mode = LexicalMode::JavaScriptTemplate;
                    index += 1;
                } else if javascript_family(language_id)
                    && bytes[index] == b'/'
                    && javascript_regex_can_start(&code[..index])
                {
                    mode = LexicalMode::JavaScriptRegex {
                        in_character_class: false,
                    };
                    index += 1;
                } else if rust_lifetime_start(language_id, bytes, index) {
                    code[index] = bytes[index];
                    index += 1;
                } else if matches!(bytes[index], b'\'' | b'"') {
                    let quote = bytes[index];
                    let triple = language_id == "python"
                        && bytes[index..].starts_with(&[quote, quote, quote]);
                    mode = LexicalMode::Quoted { quote, triple };
                    index += if triple { 3 } else { 1 };
                } else {
                    code[index] = bytes[index];
                    index += 1;
                }
            }
            LexicalMode::BlockComment { depth } => {
                if language_id == "rust" && bytes[index..].starts_with(b"/*") {
                    mode = LexicalMode::BlockComment {
                        depth: depth.saturating_add(1),
                    };
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    mode = if depth == 1 {
                        LexicalMode::Code
                    } else {
                        LexicalMode::BlockComment { depth: depth - 1 }
                    };
                    index += 2;
                } else {
                    index += 1;
                }
            }
            LexicalMode::Quoted { quote, triple } => {
                if bytes[index] == b'\\' {
                    index = skip_escaped_byte(bytes, &mut code, index);
                } else if triple && bytes[index..].starts_with(&[quote, quote, quote]) {
                    mode = LexicalMode::Code;
                    index += 3;
                } else if !triple && bytes[index] == quote {
                    mode = LexicalMode::Code;
                    index += 1;
                } else {
                    index += 1;
                }
            }
            LexicalMode::JavaScriptRegex { in_character_class } => {
                if bytes[index] == b'\\' {
                    index = skip_escaped_byte(bytes, &mut code, index);
                } else if bytes[index] == b'[' && !in_character_class {
                    mode = LexicalMode::JavaScriptRegex {
                        in_character_class: true,
                    };
                    index += 1;
                } else if bytes[index] == b']' && in_character_class {
                    mode = LexicalMode::JavaScriptRegex {
                        in_character_class: false,
                    };
                    index += 1;
                } else if bytes[index] == b'/' && !in_character_class {
                    mode = LexicalMode::Code;
                    index += 1;
                    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                        index += 1;
                    }
                } else if bytes[index] == b'\n' {
                    mode = LexicalMode::Code;
                    index += 1;
                } else {
                    index += 1;
                }
            }
            LexicalMode::JavaScriptTemplate => {
                if bytes[index] == b'\\' {
                    index = skip_escaped_byte(bytes, &mut code, index);
                } else if bytes[index] == b'`' {
                    mode = LexicalMode::Code;
                    index += 1;
                } else {
                    index += 1;
                }
            }
            LexicalMode::RustRawString { hashes } => {
                if rust_raw_string_ends(bytes, index, hashes) {
                    mode = LexicalMode::Code;
                    index += hashes + 1;
                } else {
                    index += 1;
                }
            }
        }
    }

    String::from_utf8(code).expect("masking UTF-8 source with ASCII spaces preserves UTF-8")
}

fn starts_inside_block_comment(language_id: &str, content: &str) -> bool {
    if !block_comments(language_id) {
        return false;
    }
    let first_open = content.find("/*");
    if content
        .find("*/")
        .is_some_and(|first_close| first_open.is_none_or(|first_open| first_close < first_open))
    {
        return true;
    }
    if first_open.is_some() {
        return false;
    }

    content
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty())
        .and_then(|line| line.strip_prefix('*'))
        .is_some_and(|remainder| remainder.chars().next().is_none_or(char::is_whitespace))
}

fn slash_line_comments(language_id: &str) -> bool {
    matches!(
        language_id,
        "c" | "cpp"
            | "csharp"
            | "go"
            | "gomod"
            | "java"
            | "javascript"
            | "jsx"
            | "kotlin"
            | "php"
            | "rust"
            | "scala"
            | "swift"
            | "typescript"
            | "tsx"
    )
}

fn block_comments(language_id: &str) -> bool {
    language_id == "sql" || slash_line_comments(language_id)
}

fn hash_comment_starts(language_id: &str, bytes: &[u8], index: usize) -> bool {
    if bytes[index] != b'#' {
        return false;
    }
    match language_id {
        "cmake" | "dockerfile" | "ini" | "make" | "ninja" | "php" | "properties" | "python"
        | "ruby" | "starlark" | "toml" | "yaml" => true,
        "bash" => {
            index == 0
                || bytes[index - 1].is_ascii_whitespace()
                || b";&|()".contains(&bytes[index - 1])
        }
        _ => false,
    }
}

fn alternate_line_comment_starts(language_id: &str, bytes: &[u8], index: usize) -> bool {
    let marker_matches = match language_id {
        "ini" => bytes[index] == b';',
        "properties" => bytes[index] == b'!',
        _ => false,
    };
    marker_matches
        && bytes[..index]
            .iter()
            .rev()
            .take_while(|byte| **byte != b'\n')
            .all(|byte| byte.is_ascii_whitespace())
}

fn mask_to_line_end(bytes: &[u8], code: &mut [u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    if index < bytes.len() {
        code[index] = b'\n';
        index += 1;
    }
    index
}

fn skip_escaped_byte(bytes: &[u8], code: &mut [u8], index: usize) -> usize {
    let escaped = index + 1;
    if bytes.get(escaped) == Some(&b'\n') {
        code[escaped] = b'\n';
    }
    (index + 2).min(bytes.len())
}

fn javascript_family(language_id: &str) -> bool {
    matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
}

fn javascript_regex_can_start(code: &[u8]) -> bool {
    let line_start = code
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let Some(previous) = code[line_start..]
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| line_start + index)
    else {
        return true;
    };
    if b"=([{,:;!?&|+-*%^~<>".contains(&code[previous]) {
        return true;
    }
    let prefix = &code[..=previous];
    let word_start = prefix
        .iter()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
        .map_or(0, |index| index + 1);
    JAVASCRIPT_REGEX_PREFIX_KEYWORDS.contains(&&prefix[word_start..])
}

fn rust_lifetime_start(language_id: &str, bytes: &[u8], index: usize) -> bool {
    if language_id != "rust"
        || bytes[index] != b'\''
        || index
            .checked_sub(1)
            .is_some_and(|previous| is_identifier_byte(bytes[previous]))
        || !bytes
            .get(index + 1)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return false;
    }
    let mut end = index + 2;
    while bytes.get(end).is_some_and(|byte| is_identifier_byte(*byte)) {
        end += 1;
    }

    bytes.get(end) != Some(&b'\'')
}

fn rust_raw_string_start(language_id: &str, bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    if language_id != "rust"
        || index
            .checked_sub(1)
            .is_some_and(|previous| is_identifier_byte(bytes[previous]))
    {
        return None;
    }
    let mut cursor = index;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some((cursor + 1, cursor - hashes_start))
}

fn rust_raw_string_ends(bytes: &[u8], index: usize, hashes: usize) -> bool {
    bytes.get(index) == Some(&b'"')
        && bytes
            .get(index + 1..index + 1 + hashes)
            .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
