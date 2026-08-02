use super::*;

#[test]
fn directive_parser_accepts_spacing_and_rejects_empty_directives() {
    let directive = preprocessor_directive("#  define FLAG 1").expect("define directive");

    assert_eq!(directive.keyword, "define");
    assert_eq!(directive.rest, "FLAG 1");
    assert!(preprocessor_directive("#  ").is_none());
    assert!(preprocessor_directive("define FLAG 1").is_none());
}

#[test]
fn logical_line_folding_removes_continuations_without_joining_tokens() {
    let mut logical_line = String::new();

    append_preprocessor_logical_line(&mut logical_line, "#if ENABLED \\");
    append_preprocessor_logical_line(&mut logical_line, " && READY");

    assert_eq!(logical_line, "#if ENABLED && READY");
    assert!(line_continues_preprocessor_directive("#if ENABLED \\  "));
    assert!(!line_continues_preprocessor_directive("#if ENABLED"));
}

#[test]
fn active_macro_parser_distinguishes_object_and_function_definitions() {
    let (object_name, object) =
        parse_active_macro_definition_line("#define VERSION 3").expect("object macro");
    let (function_name, function) =
        parse_active_macro_definition_line("#define WRAP(value) ((value) + 1)")
            .expect("function macro");

    assert_eq!(object_name, "VERSION");
    assert_eq!(object.replacement, "3");
    assert!(!object.function_like);
    assert_eq!(function_name, "WRAP");
    assert_eq!(function.replacement, "((value) + 1)");
    assert!(function.function_like);
}

#[test]
fn function_macro_parser_requires_parameters_replacement_and_exact_name() {
    let definition =
        parse_function_macro_definition_line("#define WRAP(value, ...) ((value) + 1)", "WRAP")
            .expect("function macro");

    assert_eq!(definition.parameters, ["value"]);
    assert_eq!(definition.replacement, "((value) + 1)");
    assert!(parse_function_macro_definition_line("#define WRAPPER(value) value", "WRAP").is_none());
    assert!(parse_function_macro_definition_line("#define WRAP() value", "WRAP").is_none());
    assert!(parse_function_macro_definition_line("#define WRAP(value)", "WRAP").is_none());
    assert!(parse_function_macro_definition_line("#define WRAP(value value", "WRAP").is_none());
}
