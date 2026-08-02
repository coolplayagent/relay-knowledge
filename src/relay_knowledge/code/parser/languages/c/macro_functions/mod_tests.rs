// Direct tests for macro-generated C function recognition.

use super::{
    LocalMacroFunctionName, MacroArgument, definition_like_macro_name,
    local_macro_generated_function_name, macro_argument_text_slots,
    macro_replacement_parameter_is_function_name,
};

#[test]
fn local_macro_recovery_distinguishes_unknown_and_non_function_macros() {
    let arguments = vec![MacroArgument {
        text: "ngx_http_demo_access".to_owned(),
        identifiers: vec!["ngx_http_demo_access".to_owned()],
    }];

    assert!(matches!(
        local_macro_generated_function_name("", "NGX_HTTP_DEMO", &arguments, 0,),
        LocalMacroFunctionName::NotMacro
    ));

    let content = "#define MODULE_ACCESS_PHASE(name) name\n";
    assert!(matches!(
        local_macro_generated_function_name(
            content,
            "MODULE_ACCESS_PHASE",
            &arguments,
            content.len(),
        ),
        LocalMacroFunctionName::Rejected
    ));
}

#[test]
fn macro_argument_slots_preserve_nested_group_commas() {
    assert_eq!(
        macro_argument_text_slots("(int, callback(a, b), {1, 2}, names[3])"),
        ["int", "callback(a, b)", "{1, 2}", "names[3]"]
    );
}

#[test]
fn definition_like_macro_names_reject_registration_and_export_surfaces() {
    assert!(definition_like_macro_name("DECLARE_FUNCTION"));
    assert!(definition_like_macro_name("HTTP_HANDLER"));
    assert!(!definition_like_macro_name("REGISTER_HANDLER"));
    assert!(!definition_like_macro_name("EXPORT_SYMBOL"));
}

#[test]
fn replacement_parameter_requires_identifier_boundary_and_return_shape() {
    assert!(macro_replacement_parameter_is_function_name(
        "static ngx_int_t name(request_t *request)",
        "name"
    ));
    assert!(!macro_replacement_parameter_is_function_name(
        "static ngx_int_t renamed(request_t *request)",
        "name"
    ));
    assert!(!macro_replacement_parameter_is_function_name(
        "target = name(request)",
        "name"
    ));
}
