use tree_sitter::{Language, Node, Parser};

use super::{dynamic_import, re_export};

#[test]
fn javascript_re_exports_require_an_ast_source_field() {
    let source = r#"
export { createClient } from "@scope/client";
export * from "./runtime.js";
export function marketplaceCopy() {
    return `Choose provider and model from the marketplace`;
}
"#;

    assert_eq!(
        javascript_re_exports("javascript", source),
        [
            "export { createClient } from \"@scope/client\";",
            "export * from \"./runtime.js\";",
        ]
    );
}

#[test]
fn typescript_re_exports_require_an_ast_source_field() {
    let source = r#"
export type { RuntimeConfig } from "@scope/runtime";
export { RuntimeController } from "./controller";
export function renderTemplate(): string {
    return `This deliberately says from inside an ordinary exported function`;
}
"#;

    assert_eq!(
        javascript_re_exports("typescript", source),
        [
            "export type { RuntimeConfig } from \"@scope/runtime\";",
            "export { RuntimeController } from \"./controller\";",
        ]
    );
}

#[test]
fn exported_function_with_large_template_is_not_an_import() {
    let template = format!(
        "export function renderMarkup() {{ return `{} from the marketplace`; }}",
        "x".repeat(40 * 1_024)
    );

    assert!(javascript_re_exports("javascript", &template).is_empty());
}

#[test]
fn dynamic_imports_remain_module_edges() {
    let source = r#"
async function loadRuntime() {
    return await import("@scope/runtime/client");
}
"#;
    let tree = parse(source, javascript_language());
    let mut imports = Vec::new();
    visit_nodes(tree.root_node(), &mut |node| {
        if let Some((module, _)) = dynamic_import("javascript", source, node) {
            imports.push(module);
        }
    });

    assert_eq!(imports, ["await import(\"@scope/runtime/client\")"]);
}

fn javascript_re_exports(language_id: &str, source: &str) -> Vec<String> {
    let language = match language_id {
        "javascript" => javascript_language(),
        "typescript" => typescript_language(),
        other => panic!("unsupported test language {other}"),
    };
    let tree = parse(source, language);
    let mut imports = Vec::new();
    visit_nodes(tree.root_node(), &mut |node| {
        if let Some((module, _)) = re_export(language_id, source, node) {
            imports.push(module);
        }
    });
    imports
}

fn parse(source: &str, language: Language) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("test grammar should be compatible");
    parser
        .parse(source, None)
        .expect("test source should parse")
}

fn visit_nodes(node: Node<'_>, visit: &mut impl FnMut(Node<'_>)) {
    visit(node);
    for index in 0..node.child_count() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        if let Some(child) = node.child(index) {
            visit_nodes(child, visit);
        }
    }
}

fn javascript_language() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}

fn typescript_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}
