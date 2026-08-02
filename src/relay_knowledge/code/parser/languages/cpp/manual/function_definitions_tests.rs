use tree_sitter::Node;

use super::*;
use crate::code::{language_metadata::detect_language, parser::syntax::parse_tree};

fn first_node_of_kind<'tree>(root: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    None
}

fn parse_cpp(content: &str) -> tree_sitter::Tree {
    let language = detect_language("src/function.cpp").expect("C++ language");
    parse_tree(language, content).expect("C++ tree")
}

#[test]
fn structured_function_symbol_uses_terminal_qualified_name() {
    let content = "int outer::Widget::Build(int value) { return value; }";
    let tree = parse_cpp(content);
    let node =
        first_node_of_kind(tree.root_node(), "function_definition").expect("function definition");

    assert_eq!(
        function_definition_symbol(content, node).map(|(name, kind, _)| (name, kind)),
        Some(("Build".to_owned(), "function"))
    );
}

#[test]
fn destructor_detection_uses_only_the_declaration_head() {
    let destructor = "Widget::~Widget() {}";
    let ordinary = "int cleanup() { widget.~Widget(); return 0; }";
    let destructor_tree = parse_cpp(destructor);
    let ordinary_tree = parse_cpp(ordinary);
    let destructor_node = first_node_of_kind(destructor_tree.root_node(), "function_definition")
        .expect("destructor definition");
    let ordinary_node = first_node_of_kind(ordinary_tree.root_node(), "function_definition")
        .expect("ordinary definition");

    assert!(function_definition_is_destructor(
        destructor,
        destructor_node
    ));
    assert!(!function_definition_is_destructor(ordinary, ordinary_node));
}

#[test]
fn decorated_function_recovery_accepts_templates_and_operators() {
    for (content, expected) in [
        (
            "__always_inline std::vector<int> BuildIds() { return {}; }",
            "BuildIds",
        ),
        (
            "__always_inline bool outer::operator==(int rhs) { return rhs != 0; }",
            "operator==",
        ),
    ] {
        let tree = parse_cpp(content);
        let node =
            first_node_of_kind(tree.root_node(), "function_definition").expect("function node");
        assert_eq!(
            gcc_decorated_function_name(content, node).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn decorated_function_recovery_rejects_malformed_heads_and_destructors() {
    for content in [
        "__always_inline 123 malformed() { return 1; }",
        "__always_inline Widget::~Widget() {}",
        "__always_inline int malformed() garbage { return 1; }",
    ] {
        let tree = parse_cpp(content);
        let node = tree.root_node().named_child(0).expect("top-level node");
        assert!(
            gcc_decorated_function_name(content, node).is_none(),
            "{content} should be rejected"
        );
    }
}
