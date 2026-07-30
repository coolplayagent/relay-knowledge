use super::*;

#[test]
fn accepts_supported_c_family_function_declarators() {
    assert!(c_family_typedef_like_function_signature(
        "ExternalResult api_call(ExternalArgument value);"
    ));
    assert!(decorated_function_head_has_recoverable_tail(
        "ExternalResult Widget::api_call(ExternalArgument value = {}) const noexcept",
        true,
        true,
        false,
    ));
    assert!(decorated_function_head_has_recovery_decorator(
        "__always_inline ExternalResult api_call(void)"
    ));
}

#[test]
fn rejects_unconsumed_or_undecorated_function_shapes() {
    assert!(!c_family_typedef_like_function_signature(
        "int api_call(void) attribute((always_inline)) garbage"
    ));
    assert!(!decorated_function_head_has_recovery_decorator(
        "int api_call(void)"
    ));
}
