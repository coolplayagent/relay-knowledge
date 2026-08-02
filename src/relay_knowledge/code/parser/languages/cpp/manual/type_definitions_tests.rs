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

#[test]
fn type_symbol_uses_real_named_specifier_and_exact_range() {
    let content = "struct Widget { int value; };";
    let language = detect_language("src/widget.cpp").expect("C++ language");
    let tree = parse_tree(language, content).expect("valid C++ tree");
    let node = first_node_of_kind(tree.root_node(), "struct_specifier").expect("struct specifier");

    let (name, kind, range) = decorated_cpp_type_symbol(content, node).expect("type symbol");

    assert_eq!((name.as_str(), kind), ("Widget", "type"));
    assert_eq!(range.byte_start, node.start_byte());
    assert_eq!(range.byte_end, node.end_byte());
    assert!(cpp_type_declaration_context(content, node));
}

#[test]
fn type_context_rejects_parameter_mentions_without_definitions() {
    let content = "int Parse(enum Direction direction);";
    let language = detect_language("src/parse.cpp").expect("C++ language");
    let tree = parse_tree(language, content).expect("valid C++ tree");
    let node = first_node_of_kind(tree.root_node(), "enum_specifier").expect("enum mention");

    assert!(!cpp_type_declaration_context(content, node));
}

#[test]
fn type_name_parsing_skips_decorators_and_keeps_qualified_tail() {
    let head = "class __declspec dllexport outer::HTTP_MODULE final";
    let tokens = cpp_head_tokens(head);
    let intro = tokens
        .iter()
        .position(|token| token.text == "class")
        .expect("class token");

    assert_eq!(
        cpp_type_name_after_intro(head, &tokens[intro + 1..]),
        Some("HTTP_MODULE")
    );
}

#[test]
fn decorated_type_head_rejects_function_parameter_type_mentions() {
    let type_head = "RK_API class Widget { int value; };";
    let function_head = "int Build(struct Request *request) { return 0; }";
    let language = detect_language("src/recovery.cpp").expect("C++ language");
    let type_tree = parse_tree(language, type_head).expect("decorated type tree");
    let function_tree = parse_tree(language, function_head).expect("function tree");
    let type_node = type_tree.root_node().named_child(0).expect("type node");
    let function_node = function_tree
        .root_node()
        .named_child(0)
        .expect("function node");

    assert!(decorated_declaration_head_starts_with_type_definition(
        type_head, type_node
    ));
    assert!(!decorated_declaration_head_starts_with_type_definition(
        function_head,
        function_node
    ));
}
