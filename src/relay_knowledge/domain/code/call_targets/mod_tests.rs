//! Direct contracts for call-target identity and callable classification.

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

#[test]
fn callable_definitions_exclude_signature_only_declarations() {
    assert!(callable_definition_symbol(
        "function",
        "int rk_c_decode(const char *input) {"
    ));
    assert!(callable_definition_symbol(
        "function",
        "int rk_c_decode(const char *input) { return 0; };"
    ));
    assert!(!callable_definition_symbol(
        "function",
        "int rk_c_decode(const char *input);"
    ));
    assert!(!callable_definition_symbol(
        "function",
        "int rk_c_decode(std::array<int, 1> input = {0});"
    ));
    assert!(!callable_definition_symbol(
        "function_declaration",
        "int rk_c_decode(const char *input)"
    ));
}
