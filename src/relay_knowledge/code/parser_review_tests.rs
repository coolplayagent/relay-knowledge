use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn c_macro_recovery_keeps_pascal_callback_and_skips_registration_macros() {
    let snapshot = parse_source_snapshot(
        "src/callbacks.c",
        br#"
DECLARE_CALLBACK(HandlerFn, Context *);
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
