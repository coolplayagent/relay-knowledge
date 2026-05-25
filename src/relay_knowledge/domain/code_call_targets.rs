pub(crate) fn call_target_name_candidates(name: &str) -> Vec<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut candidates = vec![trimmed.to_owned()];
    if let Some(leaf) = cross_language_call_leaf(trimmed)
        && leaf != trimmed
    {
        candidates.push(leaf.to_owned());
    }
    candidates
}

pub(crate) fn callable_target_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "constructor" | "function" | "function_declaration" | "macro" | "method"
    )
}

pub(crate) fn callable_definition_symbol_kind(kind: &str) -> bool {
    callable_target_symbol_kind(kind) && kind != "function_declaration"
}

fn cross_language_call_leaf(name: &str) -> Option<&str> {
    if let Some((prefix, leaf)) = name.rsplit_once('.')
        && foreign_member_prefix(prefix)
        && simple_identifier(leaf)
    {
        return Some(leaf);
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
    matches!(
        prefix_leaf,
        "C" | "bindings" | "ffi" | "libc" | "native" | "raw" | "sys"
    )
}

fn simple_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgo_and_ffi_surfaces_add_leaf_candidate() {
        assert_eq!(
            call_target_name_candidates("C.rk_c_decode"),
            ["C.rk_c_decode", "rk_c_decode"]
        );
        assert_eq!(
            call_target_name_candidates("ffi::rk_c_decode"),
            ["ffi::rk_c_decode", "rk_c_decode"]
        );
        assert_eq!(
            call_target_name_candidates("crate::ffi::rk_c_decode"),
            ["crate::ffi::rk_c_decode", "rk_c_decode"]
        );
    }

    #[test]
    fn ordinary_member_and_namespace_calls_do_not_alias_to_broad_method_names() {
        assert_eq!(
            call_target_name_candidates("client.connect"),
            ["client.connect"]
        );
        assert_eq!(
            call_target_name_candidates("module::connect"),
            ["module::connect"]
        );
    }
}
