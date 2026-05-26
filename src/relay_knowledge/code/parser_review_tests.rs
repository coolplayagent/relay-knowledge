use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn c_macro_recovery_keeps_pascal_callback_and_skips_registration_macros() {
    let snapshot = parse_source_snapshot(
        "src/callbacks.c",
        br#"
DECLARE_CALLBACK(HandlerFn, Context *);
DECLARE_CALLBACK(PlainHandlerFn, Context);
REGISTER_HANDLER(rk_registered_handler);
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "HandlerFn"),
        "PascalCase callback names should remain callable symbols: {:?}",
        snapshot.symbols
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "PlainHandlerFn"),
        "plain context callback arguments should not replace the callback symbol: {:?}",
        snapshot.symbols
    );
    assert!(
        !snapshot.symbols.iter().any(|symbol| {
            symbol.kind == "function"
                && matches!(symbol.name.as_str(), "Context" | "rk_registered_handler")
        }),
        "type arguments and registration macros should not become function definitions: {:?}",
        snapshot.symbols
    );
}

#[test]
fn c_family_recoverable_line_accepts_declspec_prefix_payloads() {
    assert!(recoverable_c_family_error_line(
        "__declspec(dllexport) class ExportedWidget {"
    ));
    assert!(recoverable_c_family_error_line(
        "__attribute__((visibility(\"default\"))) class ExportedWidget {"
    ));
    assert!(!recoverable_c_family_error_line(
        "HTTP_MODULE class ExportedWidget {"
    ));
    assert!(!recoverable_c_family_error_line(
        "struct __kernel_timespec {"
    ));
}

#[test]
fn c_family_recoverable_line_accepts_external_typedef_shapes() {
    assert!(recoverable_c_family_error_line(
        "static ngx_int_t ngx_http_demo_init(ngx_pool_t *pool)"
    ));
    assert!(recoverable_c_family_error_line(
        "ngx_module_t ngx_http_demo_module = {"
    ));
    assert!(!recoverable_c_family_error_line("ngx_pool_t *pool;"));
    assert!(!recoverable_c_family_error_line(
        "return ngx_int_t handler(req)"
    ));
    assert!(!recoverable_c_family_error_line("ngx_int_t broken_call("));
    assert!(!recoverable_c_family_error_line(
        "ngx_int_t broken_value = ;"
    ));
}

#[test]
fn c_external_header_macro_file_keeps_structured_facts() {
    let snapshot = parse_source_snapshot(
        "src/ngx_http_demo_module.c",
        br#"
#include <ngx_config.h>
#include <ngx_core.h>
#include <ngx_http.h>

#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_demo_access)
{
    return ngx_http_demo_init(request->pool);
}

static ngx_command_t ngx_http_demo_commands[] = {
    { ngx_string("demo"), NGX_HTTP_LOC_CONF, ngx_conf_set_str_slot, 0, 0, NULL },
    ngx_null_command
};

static ngx_http_module_t ngx_http_demo_module_ctx = {
    NULL,
    ngx_http_demo_init,
    NULL,
    NULL
};

ngx_module_t ngx_http_demo_module = {
    NGX_MODULE_V1,
    &ngx_http_demo_module_ctx,
    ngx_http_demo_commands,
    NGX_HTTP_MODULE,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NGX_MODULE_V1_PADDING
};
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "external typedef and macro recovery should avoid file degradation: {:?}",
        snapshot.diagnostics
    );
    assert!(snapshot.files[0].degraded_reason.is_none());
    for name in [
        "ngx_http_demo_init",
        "ngx_http_demo_access",
        "ngx_http_demo_commands",
        "ngx_http_demo_module_ctx",
        "ngx_http_demo_module",
    ] {
        assert!(
            snapshot.symbols.iter().any(|symbol| symbol.name == name),
            "{name} should be extracted as structured code graph evidence: {:?}",
            snapshot.symbols
        );
    }
    assert!(
        snapshot.calls.iter().any(|call| {
            call.caller_name.as_deref() == Some("ngx_http_demo_access")
                && call.callee_name == "ngx_http_demo_init"
        }),
        "macro-with-body definitions should own calls in their body: {:?}",
        snapshot.calls
    );
    assert!(
        snapshot.imports.iter().any(|import| {
            import.module.contains("ngx_http.h") && import.resolution_state == "unresolved"
        }),
        "external headers should remain unresolved import metadata: {:?}",
        snapshot.imports
    );
}

#[test]
fn c_macro_body_recovery_requires_definition_style_macro_name() {
    let snapshot = parse_source_snapshot(
        "src/generic_block_macro.c",
        br#"
#define MODULE_ACCESS_PHASE(name) name

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

MODULE_ACCESS_PHASE(ngx_http_demo_access)
{
    ngx_http_demo_init(0);
}
"#,
    );

    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "ngx_http_demo_access"),
        "generic block macros should not become function definitions: {:?}",
        snapshot.symbols
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "MODULE_ACCESS_PHASE"),
        "generic block macro names should not fall back to function definitions: {:?}",
        snapshot.symbols
    );
    assert!(
        !snapshot.calls.iter().any(|call| {
            call.caller_name.as_deref() == Some("ngx_http_demo_access")
                && call.callee_name == "ngx_http_demo_init"
        }),
        "generic block macros should not own call graph edges: {:?}",
        snapshot.calls
    );
}

#[test]
fn c_macro_body_recovery_reads_continued_function_macro_definition() {
    let snapshot = parse_source_snapshot(
        "src/continued_macro_module.c",
        br#"
#define KONG_ACCESS_PHASE(name) \
    static ngx_int_t name(ngx_http_request_t *request)

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_demo_access)
{
    return ngx_http_demo_init(request->pool);
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "ngx_http_demo_access"),
        "continued function macro definitions should recover the handler symbol: {:?}",
        snapshot.symbols
    );
    assert!(
        snapshot.calls.iter().any(|call| {
            call.caller_name.as_deref() == Some("ngx_http_demo_access")
                && call.callee_name == "ngx_http_demo_init"
        }),
        "continued function macro definitions should own calls in the body: {:?}",
        snapshot.calls
    );
}

#[test]
fn c_macro_body_recovery_reads_spaced_define_directive() {
    let snapshot = parse_source_snapshot(
        "src/spaced_define_macro_module.c",
        br#"
# define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_demo_access)
{
    return ngx_http_demo_init(request->pool);
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "ngx_http_demo_access"),
        "spaced define directives should recover the handler symbol: {:?}",
        snapshot.symbols
    );
    assert!(
        snapshot.calls.iter().any(|call| {
            call.caller_name.as_deref() == Some("ngx_http_demo_access")
                && call.callee_name == "ngx_http_demo_init"
        }),
        "spaced define directives should own calls in the body: {:?}",
        snapshot.calls
    );
}

#[test]
fn c_macro_body_recovery_ignores_undef_and_inactive_macro_definitions() {
    let snapshot = parse_source_snapshot(
        "src/inactive_macro_module.c",
        br#"
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#undef KONG_ACCESS_PHASE

#if 0
#define HIDDEN_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif

#ifdef NEVER_DEFINED
#define CONDITIONAL_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_after_undef)
{
    return ngx_http_demo_init(request->pool);
}

HIDDEN_PHASE(ngx_http_inactive_if)
{
    return ngx_http_demo_init(request->pool);
}

CONDITIONAL_PHASE(ngx_http_inactive_ifdef)
{
    return ngx_http_demo_init(request->pool);
}
"#,
    );

    for name in [
        "ngx_http_after_undef",
        "ngx_http_inactive_if",
        "ngx_http_inactive_ifdef",
    ] {
        assert!(
            !snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == name),
            "inactive or undefined macros should not recover handler symbols: {:?}",
            snapshot.symbols
        );
        assert!(
            !snapshot
                .calls
                .iter()
                .any(|call| call.caller_name.as_deref() == Some(name)),
            "inactive or undefined macros should not own call graph edges: {:?}",
            snapshot.calls
        );
    }
}

#[test]
fn cpp_manual_recovery_keeps_elaborated_return_type_functions() {
    let snapshot = parse_source_snapshot(
        "src/factory.cpp",
        br#"
class FactoryResult {};

class FactoryResult make_factory_result()
{
    return FactoryResult();
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "make_factory_result"),
        "elaborated return types should not hide function definitions: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_manual_recovery_keeps_prefixed_elaborated_return_type_functions() {
    let snapshot = parse_source_snapshot(
        "src/prefixed_factory.cpp",
        br#"
#define RK_API __attribute__((visibility("default")))

class FactoryResult {};

RK_API class FactoryResult make_factory_result()
{
    return FactoryResult();
}
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "make_factory_result"),
        "visibility prefixes should not route elaborated-return functions into type recovery: {:?}",
        snapshot.symbols
    );
    assert_eq!(
        snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == "class" && symbol.name == "FactoryResult")
            .count(),
        1,
        "the return type should not be emitted again as the prefixed function definition"
    );
}

#[test]
fn cpp_decorated_type_recovery_rejects_broken_member_bodies() {
    let snapshot = parse_source_snapshot(
        "include/broken_exported.hpp",
        br#"
__declspec(dllexport) class BrokenWidget {
 public:
    void Run(;
};
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Partial,
        "broken member syntax should keep diagnostics visible: symbols={:?}; diagnostics={:?}",
        snapshot.symbols,
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("error nodes")),
        "unrecoverable decorated type bodies should report parser diagnostics"
    );
}

#[test]
fn cpp_declspec_prefix_type_recovery_accepts_payload_tokens() {
    let snapshot = parse_source_snapshot(
        "include/exported.hpp",
        br#"
__declspec(dllexport) class ExportedWidget {
 public:
    void Run();
};
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "symbols={:?}; diagnostics={:?}",
        snapshot.symbols,
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "class" && symbol.name == "ExportedWidget"),
        "__declspec payload tokens should not block type recovery: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_attribute_prefix_type_recovery_accepts_payload_tokens() {
    let snapshot = parse_source_snapshot(
        "include/attribute_exported.hpp",
        br#"
__attribute__((visibility("default"))) class AttributeWidget final {
 public:
    void Run();
};
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "symbols={:?}; diagnostics={:?}",
        snapshot.symbols,
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "class" && symbol.name == "AttributeWidget"),
        "__attribute__ payload tokens should not block type recovery: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_manual_recovery_uses_terminal_qualified_type_name() {
    let content = "class A::B { int value; };";
    let language = detect_language("src/qualified.cpp").expect("C++ should be configured");
    let parsed = parse_tree(language, content).expect("C++ should parse");
    let class_node = first_node_of_kind(parsed.root_node(), "class_specifier")
        .expect("qualified class definition should be present");

    let manual = manual_definitions(content, language.id, class_node);

    assert!(
        manual
            .iter()
            .any(|(name, kind, _)| name == "B" && *kind == "class"),
        "qualified type recovery should use the terminal declared name: {:?}",
        manual
    );
    assert!(
        !manual
            .iter()
            .any(|(name, kind, _)| name == "A" && *kind == "class"),
        "qualified type recovery should not stop at the qualifier namespace/type"
    );
}

#[test]
fn cpp_manual_recovery_keeps_type_name_before_inheritance() {
    let content = "class HTTP_MODULE : public BaseModule { int value; };";
    let language = detect_language("src/inherited.cpp").expect("C++ should be configured");
    let parsed = parse_tree(language, content).expect("C++ should parse");
    let class_node = first_node_of_kind(parsed.root_node(), "class_specifier")
        .expect("inherited class definition should be present");

    let manual = manual_definitions(content, language.id, class_node);

    assert!(
        manual
            .iter()
            .any(|(name, kind, _)| name == "HTTP_MODULE" && *kind == "class"),
        "inheritance should not make a later base type replace the class name: {:?}",
        manual
    );
    assert!(
        !manual
            .iter()
            .any(|(name, kind, _)| name == "BaseModule" && *kind == "class"),
        "base classes should not become recovered class definitions"
    );
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}

fn first_node_of_kind<'tree>(
    root: tree_sitter::Node<'tree>,
    kind: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}
