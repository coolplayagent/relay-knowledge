use super::*;

fn c_tree(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .expect("C grammar should load");
    parser
        .parse(source, None)
        .expect("C source should produce a syntax tree")
}

fn named_node<'tree>(root: Node<'tree>, source: &str, kind: &str, text: &str) -> Node<'tree> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == kind && node_text(source, node) == text {
            return node;
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }

    panic!("expected {kind} node '{text}'")
}

#[test]
fn reference_nodes_and_names_accept_only_supported_c_identifier_surfaces() {
    for kind in [
        "identifier",
        "field_identifier",
        "namespace_identifier",
        "type_identifier",
    ] {
        assert!(c_family_reference_node(kind));
    }
    assert!(!c_family_reference_node("number_literal"));

    for name in ["handler", "_handler2", "SDK_TYPE"] {
        assert!(c_family_reference_name(name));
    }
    for name in ["", "2handler", "handler-name", "类型"] {
        assert!(!c_family_reference_name(name));
    }
}

#[test]
fn one_ancestor_walk_preserves_type_initializer_and_subscript_contexts() {
    let source = "\
struct Holder { Handler *handler; };
struct Ops ops = { .open = rk_open };
int value = table[index];
";
    let tree = c_tree(source);
    let root = tree.root_node();

    let handler = named_node(root, source, "type_identifier", "Handler");
    let rk_open = named_node(root, source, "identifier", "rk_open");
    let table = named_node(root, source, "identifier", "table");
    let index = named_node(root, source, "identifier", "index");

    assert_eq!(manual_reference(source, handler).unwrap().1, "type");
    assert_eq!(
        manual_reference(source, rk_open).unwrap().1,
        "implementation"
    );
    assert!(manual_reference(source, table).is_none());
    assert_eq!(manual_reference(source, index).unwrap().1, "implementation");
}
