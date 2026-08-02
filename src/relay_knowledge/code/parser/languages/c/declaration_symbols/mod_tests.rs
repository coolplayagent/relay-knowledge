// Direct tests for C declaration symbol materialization.

use super::{direct_function_definition_symbol, top_level_declaration_symbols};

fn c_tree(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .expect("C grammar should load");
    parser
        .parse(source, None)
        .expect("C source should produce a syntax tree")
}

#[test]
fn top_level_declarations_materialize_types_data_and_direct_functions() {
    let source = "\
typedef struct Handler { int id; } Handler;
enum State { READY } state;
static struct Ops ops = {0};
int open_file(int fd);
void (*callback)(int);
";
    let tree = c_tree(source);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let symbols = root
        .named_children(&mut cursor)
        .filter(|node| matches!(node.kind(), "type_definition" | "declaration"))
        .flat_map(|node| top_level_declaration_symbols(source, node))
        .map(|(name, kind, _)| (name, kind))
        .collect::<Vec<_>>();

    assert_eq!(
        symbols,
        [
            ("Handler".to_owned(), "type"),
            ("State".to_owned(), "type"),
            ("ops".to_owned(), "constant"),
            ("open_file".to_owned(), "function_declaration"),
        ]
    );
}

#[test]
fn direct_function_definition_materializes_name_kind_and_body_range() {
    let source = "static int run(void) {\n    return 0;\n}";
    let tree = c_tree(source);
    let node = tree
        .root_node()
        .named_child(0)
        .expect("function definition should be present");
    let (name, kind, range) =
        direct_function_definition_symbol(source, node).expect("function should materialize");

    assert_eq!(name, "run");
    assert_eq!(kind, "function");
    assert_eq!((range.line_start, range.line_end), (1, 3));
    assert_eq!(&source[range.byte_start..range.byte_end], source);
}
