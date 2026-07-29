use super::*;

#[test]
fn local_macro_lookup_accepts_spaced_define_directives() {
    let content = "# define KONG_ACCESS_PHASE(name) \\\n    static ngx_int_t name(ngx_http_request_t *request)\n";
    let LocalFunctionMacroDefinition::Function(definition) =
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
    else {
        panic!("spaced define directive should be visible");
    };

    assert_eq!(definition.parameters, ["name"]);
    assert!(definition.replacement.contains("name("));
}

#[test]
fn local_macro_lookup_respects_undef() {
    let content = "\
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#undef KONG_ACCESS_PHASE
";

    assert!(matches!(
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
        LocalFunctionMacroDefinition::Unavailable
    ));
}

#[test]
fn local_macro_lookup_ignores_inactive_branches() {
    let content = "\
#if 0
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#if FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#ifdef NEVER_DEFINED
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

    assert!(matches!(
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
        LocalFunctionMacroDefinition::Unavailable
    ));
}

#[test]
fn local_macro_lookup_evaluates_numeric_macro_conditions() {
    let disabled = "\
#define FEATURE_FLAG 0
#if FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

    assert!(matches!(
        local_function_macro_definition(disabled, "KONG_ACCESS_PHASE", disabled.len()),
        LocalFunctionMacroDefinition::Unavailable
    ));

    let enabled_by_negation = "\
#define FEATURE_FLAG 0
#if !FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    let LocalFunctionMacroDefinition::Function(definition) = local_function_macro_definition(
        enabled_by_negation,
        "KONG_ACCESS_PHASE",
        enabled_by_negation.len(),
    ) else {
        panic!("numeric macro expansion should make !FEATURE_FLAG active");
    };

    assert_eq!(definition.parameters, ["name"]);
}

#[test]
fn local_macro_lookup_requires_complete_defined_conditions() {
    let missing_rhs = "\
#define ENABLE_A 1
#if defined(ENABLE_A) && defined(ENABLE_B)
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

    assert!(matches!(
        local_function_macro_definition(missing_rhs, "KONG_ACCESS_PHASE", missing_rhs.len()),
        LocalFunctionMacroDefinition::Unavailable
    ));

    let complete_condition = "\
#define ENABLE_A 1
#define ENABLE_B 1
#if defined(ENABLE_A) && defined(ENABLE_B)
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    let LocalFunctionMacroDefinition::Function(definition) = local_function_macro_definition(
        complete_condition,
        "KONG_ACCESS_PHASE",
        complete_condition.len(),
    ) else {
        panic!("complete defined conjunction should activate the branch");
    };

    assert_eq!(definition.parameters, ["name"]);
}

#[test]
fn local_macro_lookup_parses_standard_numeric_constants() {
    let content = "\
#if 1U && 0x1 && (1) && 1 /* comment */
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    let LocalFunctionMacroDefinition::Function(definition) =
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
    else {
        panic!("standard numeric constants should activate the branch");
    };

    assert_eq!(definition.parameters, ["name"]);
}

#[test]
fn local_macro_lookup_evaluates_comparison_conditions() {
    let content = "\
#define FEATURE_FLAG 1
#define VERSION 3
#if FEATURE_FLAG == 1 && VERSION >= 2
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    let LocalFunctionMacroDefinition::Function(definition) =
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
    else {
        panic!("comparison conditions should activate matching branches");
    };

    assert_eq!(definition.parameters, ["name"]);

    let inactive = "\
#define VERSION 3
#if VERSION < 2
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    assert!(matches!(
        local_function_macro_definition(inactive, "KONG_ACCESS_PHASE", inactive.len()),
        LocalFunctionMacroDefinition::Unavailable
    ));
}

#[test]
fn local_macro_lookup_joins_continued_if_conditions() {
    let content = "\
#define FEATURE_FLAG 1
#define EXTRA_FLAG 1
#if FEATURE_FLAG \\
    && EXTRA_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
    let LocalFunctionMacroDefinition::Function(definition) =
        local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
    else {
        panic!("continued #if conditions should activate matching branches");
    };

    assert_eq!(definition.parameters, ["name"]);
}
