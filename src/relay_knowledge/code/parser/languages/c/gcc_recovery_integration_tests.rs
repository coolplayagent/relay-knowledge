use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn gcc_recovery_tracks_multiline_body_delimiters() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/multiline_policy.c",
        br#"
int pdp_helper(int value, int marker)
{
    return value + marker;
}

attribute((always_inline)) int pdp_multiline_policy(int value)
{
    return pdp_helper(
        value,
        '['
    );
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "multi-line valid statements should not block decorated recovery: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_multiline_policy")
    );
}

#[test]
fn gcc_recovery_requires_decorator_for_function_error() {
    let snapshot = parse_source_snapshot(
        "src/plain_broken.c",
        br#"
int valid_plain_symbol(void)
{
    return 1;
}

int broken_plain_function(int left,, int right) @
{
    return left + right;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Partial,
        "ordinary malformed function syntax must not be recovered as a compiler extension"
    );
    assert!(
        !recoverable_decorated_function_error_text(
            "int broken_plain_function(int left,, int right) { return left + right; }"
        ),
        "function ERROR recovery must require a compiler-extension decorator"
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "broken_plain_function"),
        "ordinary malformed C ERROR nodes must not be indexed as recovered functions: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_ignores_decorator_tokens_inside_parameters() {
    let snapshot = parse_source_snapshot(
        "src/plain_parameter_name.c",
        br#"
int valid_plain_symbol(void)
{
    return 1;
}

int broken_parameter_name(int attribute,, int right) @
{
    return right;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Partial,
        "parameter names that match decorator tokens must not prove GCC recovery"
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "broken_parameter_name"),
        "ordinary malformed C ERROR nodes must not be indexed as decorated functions: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_ignores_literals_in_attribute_payloads() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/annotated_policy.c",
        br#"
__attribute__((annotate("("))) int pdp_annotated_policy(void)
{
    return 1;
}

__declspec(dllexport) int pdp_exported_policy(void)
{
    return 1;
}

__attribute__((unused)) int pdp_unused_policy(void)
{
    return 1;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "attribute payload literals should not hide the real function parameter list: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_annotated_policy")
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_exported_policy"),
        "decorator payload tokens before C return types should be accepted: {:?}",
        snapshot.symbols
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_unused_policy"),
        "parsed C declarators should still be indexed when narrow recovery rejects a decorator: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_finds_body_after_attribute_payload_braces() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/brace_annotated_policy.c",
        br#"
__attribute__((annotate("{"))) int pdp_brace_annotated_policy(void)
{
    return 1;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "attribute payload braces should not hide the real function body: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_brace_annotated_policy"),
        "decorated function symbol should be recovered from the real body head: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_keeps_postfix_attribute_payload_scanning_literal_aware() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/postfix_literal_policy.c",
        br#"
int pdp_postfix_literal_policy(void) attribute((annotate(")")))
{
    return 1;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "postfix attribute payload literals should not truncate the attribute tail: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot.symbols.iter().any(|symbol| {
            symbol.kind == "function" && symbol.name == "pdp_postfix_literal_policy"
        }),
        "postfix literal payload function should be recovered: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_handles_line_comments_in_multiline_parameter_lists() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/commented_params.c",
        br#"
__always_inline int pdp_commented_parameters(
    int left, // primary value
    int right
)
{
    return left + right;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "line comments in multiline parameter lists should not truncate recovery scans: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_commented_parameters"),
        "commented multiline parameter function should be recovered: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_decorated_type_recovery_tracks_multiline_member_delimiters() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/multiline_widget.hpp",
        br#"
#define RK_API __attribute__((visibility("default")))

RK_API class PdpMultilineWidget {
 public:
    void update(
        int value,
        const char *name
    );
};
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "multiline member declarations should not block decorated type recovery: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "class" && symbol.name == "PdpMultilineWidget"),
        "decorated class symbol should still be recovered: {:?}",
        snapshot.symbols
    );
}

#[test]
fn gcc_recovery_accepts_labels_and_multiline_comments() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/labeled_policy.c",
        br#"
attribute((always_inline)) int pdp_labeled_policy(int token)
{
    /* explain
     * branch mapping
     */
    switch (token) {
    case 1:
        return token;
    default:
        goto done;
    }
done:
    return 0;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "labels and multiline comments should not block decorated recovery: {:?}",
        snapshot.diagnostics
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "pdp_labeled_policy")
    );
}

#[test]
fn cpp_gcc_recovery_accepts_suffix_qualifiers_and_default_arguments() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/qualified_policy.cpp",
        br#"
namespace ns {
class Policy {
 public:
    int size() const;
};
}

__always_inline int ns::Policy::size() const
{
    return 1;
}

__always_inline int limit_for(int value) noexcept
{
    if (value) {
    }
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "C++ default arguments and suffix qualifiers should remain recoverable: {:?}",
        snapshot.diagnostics
    );
    for name in ["size", "limit_for"] {
        assert!(
            snapshot.symbols.iter().any(|symbol| {
                matches!(symbol.kind.as_str(), "function" | "method") && symbol.name == name
            }),
            "{name} should be recovered from decorated C++ code: {:?}",
            snapshot.symbols
        );
    }
    assert!(
        recoverable_decorated_function_error_text(
            "__always_inline int limit_for(int value = 1) noexcept { return value; }"
        ),
        "decorated C++ recovery should allow default values inside the parameter list"
    );
    assert!(
        recoverable_decorated_function_error_text(
            "__always_inline int limit_for(const char *value = \")\") { return value[0]; }"
        ),
        "default-argument literals should not truncate the parameter list"
    );
}

#[test]
fn cpp_gcc_recovery_requires_decorator_for_error_functions() {
    let snapshot = parse_source_snapshot(
        "src/plain_broken.cpp",
        br#"
int valid_cpp_symbol()
{
    return 1;
}

int broken_cpp_function(int left,, int right) @
{
    return left + right;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Partial,
        "ordinary malformed C++ functions must not be recovered as compiler extensions"
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "broken_cpp_function"),
        "ordinary malformed C++ ERROR nodes must not be indexed as recovered functions: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_gcc_recovery_accepts_template_return_types_and_operators() {
    let source = br#"
namespace ns {
class Policy {};
}

__always_inline std::vector<int> BuildPolicyIds()
{
    return {};
}

__always_inline bool ns::operator==(const ns::Policy &rhs)
{
    return &rhs != nullptr;
}
"#;
    let snapshot = parse_source_snapshot("external_deps/sdk/template_operator_policy.cpp", source);

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "template return types and operator declarators should remain recoverable: {:?}",
        snapshot.diagnostics
    );
    for name in ["BuildPolicyIds", "operator=="] {
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == name),
            "{name} should be recovered from decorated C++ code: {:?}",
            snapshot.symbols
        );
    }
}

#[test]
fn gcc_recovery_handles_literal_payloads_and_rejects_type_body_expressions() {
    let function_snapshot = parse_source_snapshot(
        "external_deps/sdk/key_value_policy.c",
        br#"
__attribute__((annotate("key=value"))) int key_value_policy(void)
{
    return 1;
}
"#,
    );
    assert_eq!(
        function_snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "attribute payload literals should not look like initializer syntax: {:?}",
        function_snapshot.diagnostics
    );
    assert!(
        function_snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "function" && symbol.name == "key_value_policy" })
    );

    let type_snapshot = parse_source_snapshot(
        "external_deps/sdk/literal_widget.hpp",
        br#"
__attribute__((annotate("{"))) class LiteralWidget {
 public:
    int value;
};
"#,
    );
    assert_eq!(
        type_snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "type attribute payload braces should not hide the real body: {:?}",
        type_snapshot.diagnostics
    );
    assert!(
        type_snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "class" && symbol.name == "LiteralWidget")
    );

    assert!(
        !recoverable_decorated_type_error_text(
            "RK_API class BrokenWidget { public: int value; value++; };"
        ),
        "expression statements inside decorated type bodies must not be recoverable"
    );

    let invalid_inline_body_snapshot = parse_source_snapshot(
        "include/broken_inline_exported.hpp",
        br#"
RK_API class BrokenWidget {
 public:
    int Run() { return @; }
};
"#,
    );
    assert_eq!(
        invalid_inline_body_snapshot.files[0].parse_status,
        CodeParseStatus::Partial,
        "invalid inline method bodies should keep diagnostics visible: symbols={:?}; diagnostics={:?}",
        invalid_inline_body_snapshot.symbols,
        invalid_inline_body_snapshot.diagnostics
    );
}

#[test]
fn cpp_gcc_recovery_scans_literals_and_skips_destructor_symbols() {
    let literal_snapshot = parse_source_snapshot(
        "external_deps/sdk/cpp_literal_policy.cpp",
        br#"
__attribute__((annotate("("))) int annotated_cpp_policy()
{
    return 1;
}
"#,
    );
    assert_eq!(
        literal_snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "C++ symbol recovery should ignore literal parentheses in attributes: {:?}",
        literal_snapshot.diagnostics
    );
    assert!(
        literal_snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.kind == "function" && symbol.name == "annotated_cpp_policy" })
    );

    let destructor_snapshot = parse_source_snapshot(
        "external_deps/sdk/destructor_policy.cpp",
        br#"
namespace ns {
class Policy {
 public:
    ~Policy();
};
}

__always_inline ns::Policy::~Policy()
{
}

void cleanup()
{
    log("~cleanup");
}

void cleanup_inline() { log("Policy::~Policy"); }
"#,
    );
    assert!(
        !destructor_snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "Policy"),
        "decorated destructors must not be indexed as class-named functions: {:?}",
        destructor_snapshot.symbols
    );
    assert!(
        destructor_snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "cleanup"),
        "normal functions mentioning destructor-like text in the body should still be indexed: {:?}",
        destructor_snapshot.symbols
    );
    assert!(
        destructor_snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "cleanup_inline"),
        "one-line functions mentioning destructor-like text in the signature excerpt should still be indexed: {:?}",
        destructor_snapshot.symbols
    );
}

#[test]
fn gcc_recovery_rejects_malformed_parameter_lists_and_c_suffixes() {
    let c_snapshot = parse_source_snapshot(
        "external_deps/sdk/malformed_tail.c",
        br#"
int valid_c_symbol(void) { return 1; }

int broken_plain_prototype(void)

int postfix_garbage(void) attribute((always_inline)) garbage
{
    return 1;
}

__always_inline int broken_slots(int left,, int right)
{
    return left + right;
}

__always_inline 123 malformed_head(void)
{
    return 1;
}

__always_inline int c_method_suffix(void) const
{
    return 1;
}

__always_inline int c_operator_syntax(int value)
{
    return value;
}

__always_inline int operator+(int value)
{
    return value;
}

__always_inline int invalid_body_token(void)
{
    return @;
}

__always_inline int pending_assignment(void)
{
    int value = // missing initializer
    ;
    return value;
}
"#,
    );

    assert_eq!(c_snapshot.files[0].parse_status, CodeParseStatus::Partial);
    for name in [
        "postfix_garbage",
        "broken_slots",
        "malformed_head",
        "c_method_suffix",
        "operator+",
        "invalid_body_token",
        "pending_assignment",
    ] {
        assert!(
            !c_snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == name),
            "{name} should not be recovered from malformed C syntax: {:?}",
            c_snapshot.symbols
        );
    }
    assert!(
        !recoverable_decorated_function_error_text(
            "__always_inline int invalid_body_token(void) { return @; }"
        ),
        "decorated function recovery must reject invalid body tokens"
    );
    assert!(
        !recoverable_decorated_function_error_text(
            "__always_inline int pending_assignment(void) { int value = // missing initializer\n; return value; }"
        ),
        "decorated function recovery must carry pending assignments across body lines"
    );
    assert!(
        c_snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "c_operator_syntax"),
        "ordinary valid C functions near invalid operator syntax should still be indexed: {:?}",
        c_snapshot.symbols
    );
    assert!(
        !recoverable_c_family_error_line("int broken_plain_prototype(void)"),
        "plain builtin prototypes without a terminator are ordinary syntax errors"
    );
}

#[test]
fn cpp_gcc_recovery_validates_tails_and_conversion_operators() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/conversion_policy.cpp",
        br#"
namespace ns {
class Policy {};
}

__always_inline ns::Policy::operator bool() const
{
    return true;
}

__always_inline int malformed_tail() garbage
{
    return 1;
}

__always_inline int malformed_slots(int left,, int right)
{
    return left + right;
}

__always_inline int invalid_body_token()
{
    return @;
}
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Partial);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "function" && symbol.name == "operator bool"),
        "conversion operator should be recovered: {:?}",
        snapshot.symbols
    );
    for name in ["malformed_tail", "malformed_slots", "invalid_body_token"] {
        assert!(
            !snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == name),
            "{name} should not be indexed from malformed C++ syntax: {:?}",
            snapshot.symbols
        );
    }
}

#[test]
fn gcc_recovery_accepts_arbitrary_attribute_payload_tokens() {
    let snapshot = parse_source_snapshot(
        "external_deps/sdk/attribute_payloads.c",
        br#"
__attribute__((nonnull(1))) int accepts_nonnull(char *value)
{
    return value != 0;
}

__attribute__((section(".init"))) int accepts_section(void)
{
    return 1;
}
"#,
    );

    assert_eq!(
        snapshot.files[0].parse_status,
        CodeParseStatus::Parsed,
        "valid attribute payloads should not block recovery: {:?}",
        snapshot.diagnostics
    );
    for name in ["accepts_nonnull", "accepts_section"] {
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.kind == "function" && symbol.name == name),
            "{name} should be recovered from valid GCC attribute payloads: {:?}",
            snapshot.symbols
        );
    }
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
