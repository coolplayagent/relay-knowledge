//! Owns cross-language call-target candidate and callable-definition policy.

pub(crate) fn call_target_name_candidates(name: &str, path: &str) -> Vec<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut candidates = vec![trimmed.to_owned()];
    if let Some(leaf) = cross_language_call_leaf(trimmed, path)
        && leaf != trimmed
    {
        candidates.push(leaf.to_owned());
    }
    candidates
}

pub(crate) fn callable_target_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "constructor" | "function" | "function_declaration" | "macro" | "method"
    )
}

pub(crate) fn callable_definition_symbol(kind: &str, signature: &str) -> bool {
    callable_target_symbol_kind(kind) && !callable_declaration_symbol(kind, signature)
}

fn callable_declaration_symbol(kind: &str, signature: &str) -> bool {
    kind == "function_declaration" || signature_only_callable(kind, signature)
}

fn signature_only_callable(kind: &str, signature: &str) -> bool {
    matches!(kind, "constructor" | "function" | "method")
        && signature.trim_end().ends_with(';')
        && !contains_top_level_body_block(signature)
}

fn contains_top_level_body_block(signature: &str) -> bool {
    let mut parenthesis_depth = 0_u16;
    let mut bracket_depth = 0_u16;
    for character in signature.chars() {
        match character {
            '(' => parenthesis_depth = parenthesis_depth.saturating_add(1),
            ')' => parenthesis_depth = parenthesis_depth.saturating_sub(1),
            '[' => bracket_depth = bracket_depth.saturating_add(1),
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' if parenthesis_depth == 0 && bracket_depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn cross_language_call_leaf<'a>(name: &'a str, path: &str) -> Option<&'a str> {
    if let Some((prefix, leaf)) = name.rsplit_once('.')
        && simple_identifier(leaf)
    {
        if prefix == "C" && go_source_path(path) {
            return Some(leaf);
        }
        if prefix != "C" && simple_identifier(prefix) && foreign_member_prefix(prefix) {
            return Some(leaf);
        }
    }
    if let Some((prefix, leaf)) = name.rsplit_once("::")
        && foreign_member_prefix(prefix)
        && simple_identifier(leaf)
    {
        return Some(leaf);
    }
    None
}

fn foreign_member_prefix(prefix: &str) -> bool {
    let prefix_leaf = prefix
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .unwrap_or(prefix);
    matches!(prefix_leaf, "bindings" | "ffi" | "libc")
        || prefix_leaf
            .strip_suffix("_sys")
            .is_some_and(|crate_name| !crate_name.is_empty())
}

fn go_source_path(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("go"))
}

fn simple_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod mod_tests;
