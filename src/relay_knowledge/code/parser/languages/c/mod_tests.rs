use super::*;

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
