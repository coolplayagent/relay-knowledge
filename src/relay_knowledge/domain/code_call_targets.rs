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

pub(crate) fn callable_definition_symbol_kind(kind: &str) -> bool {
    callable_target_symbol_kind(kind) && kind != "function_declaration"
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
mod tests {
    use super::*;

    #[test]
    fn cgo_and_ffi_surfaces_add_leaf_candidate() {
        assert_eq!(
            call_target_name_candidates("C.rk_c_decode", "bridge/go_bridge.go"),
            ["C.rk_c_decode", "rk_c_decode"]
        );
        assert_eq!(
            call_target_name_candidates("ffi::rk_c_decode", "src/lib.rs"),
            ["ffi::rk_c_decode", "rk_c_decode"]
        );
        assert_eq!(
            call_target_name_candidates("crate::ffi::rk_c_decode", "src/lib.rs"),
            ["crate::ffi::rk_c_decode", "rk_c_decode"]
        );
        assert_eq!(
            call_target_name_candidates("openssl_sys::rk_c_decode", "src/lib.rs"),
            ["openssl_sys::rk_c_decode", "rk_c_decode"]
        );
    }

    #[test]
    fn ordinary_member_and_namespace_calls_do_not_alias_to_broad_names() {
        assert_eq!(
            call_target_name_candidates("client.connect", "src/lib.rs"),
            ["client.connect"]
        );
        assert_eq!(
            call_target_name_candidates("module::connect", "src/lib.rs"),
            ["module::connect"]
        );
        assert_eq!(
            call_target_name_candidates("module::sys::connect", "src/lib.rs"),
            ["module::sys::connect"]
        );
        assert_eq!(
            call_target_name_candidates("module.raw.connect", "src/lib.rs"),
            ["module.raw.connect"]
        );
        assert_eq!(
            call_target_name_candidates("client.ffi.connect", "src/lib.rs"),
            ["client.ffi.connect"]
        );
        assert_eq!(
            call_target_name_candidates("std::ffi::CString::new", "src/lib.rs"),
            ["std::ffi::CString::new"]
        );
        assert_eq!(
            call_target_name_candidates("C.connect", "src/lib.rs"),
            ["C.connect"]
        );
        assert_eq!(
            call_target_name_candidates("obj.C.connect", "bridge/go_bridge.go"),
            ["obj.C.connect"]
        );
    }
}
