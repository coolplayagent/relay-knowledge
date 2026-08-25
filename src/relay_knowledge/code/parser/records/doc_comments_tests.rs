use super::{
    MAX_DOC_BLOCK_BYTES, MAX_DOC_BLOCK_LINES, MAX_TYPE_DOC_BLOCK_SCAN_BYTES, doc_comment_before,
};
use crate::{
    code::{SnapshotBuild, parse_indexed_file},
    domain::{CodeIndexSnapshot, CodeRepositoryRegistration},
};

fn parse_source(path: &str, source: &str) -> CodeIndexSnapshot {
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
    parse_indexed_file(&mut build, path, source.as_bytes()).expect("source should parse");
    build.finish()
}

#[test]
fn java_parser_persists_javadoc_as_symbol_documentation() {
    let snapshot = parse_source(
        "src/Dispatcher.java",
        "/** Front controller for web requests. */\npublic class Dispatcher {}\n",
    );
    let dispatcher = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Dispatcher")
        .expect("class symbol should be extracted");

    assert_eq!(
        dispatcher.doc_comment.as_deref(),
        Some("Front controller for web requests.")
    );
}

#[test]
fn c_function_doc_uses_declaration_owner_without_expanding_symbol_range() {
    let source = "/** Opens a resource with bounded retries. */\nint open_resource(const char *path) { return path != 0; }\n";
    let snapshot = parse_source("src/resource.c", source);
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "open_resource")
        .expect("C function symbol should be extracted");

    assert_eq!(
        symbol.doc_comment.as_deref(),
        Some("Opens a resource with bounded retries.")
    );
    assert_eq!(
        symbol.byte_range.start as usize,
        source
            .find("int open_resource")
            .expect("function declaration should exist")
    );
    assert!(symbol.signature.starts_with("int open_resource"));
    assert!(!symbol.signature.contains("Opens a resource"));
}

#[test]
fn cpp_template_doc_uses_outer_owner_without_expanding_declarator_range() {
    let source = "/** Converts a value without changing its type. */\ntemplate <typename T>\nT convert_value(T value);\n";
    let snapshot = parse_source("src/convert.cpp", source);
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "convert_value")
        .expect("C++ template function symbol should be extracted");

    assert_eq!(
        symbol.doc_comment.as_deref(),
        Some("Converts a value without changing its type.")
    );
    assert_eq!(
        symbol.byte_range.start as usize,
        source
            .find("convert_value")
            .expect("function declarator should exist")
    );
    assert!(symbol.signature.starts_with("convert_value(T value)"));
    assert!(!symbol.signature.contains("template"));
}

#[test]
fn java_annotation_remains_part_of_the_documented_declaration_owner() {
    let source = "/** Legacy endpoint retained for compatibility. */\n@Deprecated\npublic class LegacyEndpoint {}\n";
    let snapshot = parse_source("src/LegacyEndpoint.java", source);
    let symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "LegacyEndpoint")
        .expect("annotated Java class should be extracted");

    assert_eq!(
        symbol.doc_comment.as_deref(),
        Some("Legacy endpoint retained for compatibility.")
    );
}

#[test]
fn adjacent_javadoc_is_normalized_for_the_next_declaration() {
    let source = "/**\n * Front controller servlet.\n * Dispatches web requests.\n */\npublic class Dispatcher {}\n";
    let declaration = source
        .find("public class")
        .expect("declaration should exist");

    assert_eq!(
        doc_comment_before(source, declaration, "java", "class").as_deref(),
        Some("Front controller servlet.\nDispatches web requests.")
    );
}

#[test]
fn ordinary_block_comment_is_not_documentation() {
    let source = "/* implementation note */\npublic class Worker {}\n";
    let declaration = source
        .find("public class")
        .expect("declaration should exist");

    assert_eq!(
        doc_comment_before(source, declaration, "java", "class"),
        None
    );
}

#[test]
fn crlf_and_outer_doc_blocks_are_utf8_safe() {
    let source = "/**\r\n * 处理请求。\r\n * 保持顺序。\r\n */\r\npub struct Queue;\r\n";
    let declaration = source.find("pub struct").expect("declaration should exist");

    assert_eq!(
        doc_comment_before(source, declaration, "rust", "struct").as_deref(),
        Some("处理请求。\n保持顺序。")
    );
}

#[test]
fn rust_inner_doc_block_is_not_attached_to_the_next_symbol() {
    let source = "/*! Documents the containing module, not Queue. */\npub struct Queue;\n";
    let snapshot = parse_source("src/lib.rs", source);
    let queue = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Queue")
        .expect("Rust struct should be extracted");

    assert_eq!(queue.doc_comment, None);
}

#[test]
fn doc_block_binds_only_to_the_nearest_declaration() {
    let source = "/** Describes First. */\nclass First {}\nclass Second {}\n";
    let first = source
        .find("class First")
        .expect("first class should exist");
    let second = source
        .find("class Second")
        .expect("second class should exist");

    assert_eq!(
        doc_comment_before(source, first, "java", "class").as_deref(),
        Some("Describes First.")
    );
    assert_eq!(doc_comment_before(source, second, "java", "class"), None);
}

#[test]
fn long_type_javadoc_keeps_a_bounded_utf8_safe_leading_summary() {
    let details = (0..90)
        .map(|index| {
            format!(
                " * Detailed controller route {index:02} keeps framework dispatch observable 界界界界界界界界.\n"
            )
        })
        .collect::<String>();
    let source = format!(
        "/**\n * Central front controller servlet dispatches web requests through an MVC framework.\n{details} */\n@SuppressWarnings(\"serial\")\npublic class GatewayController {{}}\n"
    );
    let snapshot = parse_source("src/GatewayController.java", &source);
    let gateway = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "GatewayController")
        .expect("documented class should be extracted");
    let documentation = gateway
        .doc_comment
        .as_deref()
        .expect("bounded type documentation should be retained");

    assert!(documentation.starts_with("Central front controller servlet"));
    assert!(documentation.len() <= MAX_DOC_BLOCK_BYTES);
    assert!(documentation.lines().count() <= MAX_DOC_BLOCK_LINES);
    assert!(documentation.is_char_boundary(documentation.len()));
}

#[test]
fn oversized_non_type_doc_blocks_remain_outside_the_bounded_scan() {
    let source = format!(
        "/** {} */\nvoid run_worker() {{}}\n",
        "界".repeat(MAX_DOC_BLOCK_BYTES)
    );
    let declaration = source
        .find("void run_worker")
        .expect("declaration should exist");

    assert_eq!(
        doc_comment_before(&source, declaration, "java", "method"),
        None
    );
}

#[test]
fn doc_block_lookback_bounds_whitespace_at_a_utf8_boundary() {
    let source = format!(
        "/**\u{754c}*/{}class TooFar {{}}\n",
        " ".repeat(MAX_TYPE_DOC_BLOCK_SCAN_BYTES)
    );
    let declaration = source
        .find("class TooFar")
        .expect("declaration should exist");

    assert_eq!(
        doc_comment_before(&source, declaration, "java", "class"),
        None
    );
}

#[test]
fn existing_line_doc_behavior_is_preserved() {
    let source = "/// Runs the worker.\nfn run_worker() {}\n";
    let declaration = source
        .find("fn run_worker")
        .expect("declaration should exist");

    assert_eq!(
        doc_comment_before(source, declaration, "rust", "function").as_deref(),
        Some("Runs the worker.")
    );
}
