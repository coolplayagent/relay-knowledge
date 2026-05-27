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
    return value;
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
