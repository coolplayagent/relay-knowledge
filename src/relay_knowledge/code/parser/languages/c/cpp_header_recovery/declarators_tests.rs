//! C++ header declarator contract tests.

use super::{cpp_class_header_name, member_function_declaration_name};

#[test]
fn class_declarators_skip_export_and_alignment_decorators() {
    assert_eq!(
        cpp_class_header_name("LEVELDB_EXPORT class alignas(8) Visible final {"),
        Some("Visible".to_owned())
    );
    assert_eq!(cpp_class_header_name("enum class State {"), None);
}

#[test]
fn member_declarators_accept_attributes_before_function_names() {
    assert_eq!(
        member_function_declaration_name(
            "__attribute__((warn_unused_result)) Status Open(const char *value);"
        ),
        Some("Open".to_owned())
    );
}

#[test]
fn member_declarators_reject_non_callable_or_unsupported_surfaces() {
    for declaration in [
        "void Removed() = delete;",
        "Widget() = default;",
        "Widget::~Widget();",
        "Status operator()();",
        "void (*callback)(int);",
        "int count;",
    ] {
        assert_eq!(
            member_function_declaration_name(declaration),
            None,
            "{declaration}"
        );
    }
}
