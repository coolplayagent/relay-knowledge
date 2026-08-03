// Direct tests for C declaration symbol materialization.

use super::{
    direct_composite_type_symbol, direct_function_definition_symbol, top_level_declaration_symbols,
};

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

#[test]
fn direct_enum_specifier_materializes_its_tag_type() {
    let source = "enum Direction { kForward, kReverse };";
    let tree = c_tree(source);
    let node = tree
        .root_node()
        .named_child(0)
        .expect("enum specifier should be present");

    let (name, kind, range) =
        direct_composite_type_symbol(source, node).expect("enum tag should materialize");

    assert_eq!((name.as_str(), kind), ("Direction", "type"));
    assert_eq!(
        &source[range.byte_start..range.byte_end],
        "enum Direction { kForward, kReverse }"
    );
}

#[test]
fn direct_struct_specifier_materializes_its_tag_type() {
    let source = "#ifndef RK_DRIVER_OPS_H\nstruct rk_driver_ops { int (*read)(void); };\n#endif";
    let tree = c_tree(source);
    let mut stack = vec![tree.root_node()];
    let mut node = None;
    while let Some(candidate) = stack.pop() {
        if candidate.kind() == "struct_specifier" {
            node = Some(candidate);
            break;
        }
        for index in (0..candidate.named_child_count()).rev() {
            let index = u32::try_from(index).expect("named-child index should fit u32");
            stack.push(candidate.named_child(index).expect("named child"));
        }
    }
    let node = node.expect("struct specifier should parse inside a preprocessor guard");

    let (name, kind, range) = direct_composite_type_symbol(source, node)
        .expect("top-level struct tag should materialize");

    assert_eq!(name, "rk_driver_ops");
    assert_eq!(kind, "type");
    assert_eq!(range.line_start, 2);
}
