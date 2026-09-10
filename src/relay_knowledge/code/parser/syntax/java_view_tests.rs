use super::*;

#[test]
fn java_identifier_view_preserves_original_ranges_and_real_syntax_errors() {
    let language = crate::code::languages::detect_language("App.java").unwrap();
    let cr_comment = "// comment\rclass €Type {}";
    assert!(
        !super::super::parse_tree(language, cr_comment)
            .unwrap()
            .root_node()
            .has_error()
    );
    let cr_tree = super::super::parse_tree(language, cr_comment).unwrap();
    assert!(
        !crate::code::java_namespace::collect(cr_tree.root_node(), cr_comment)
            .evidence
            .complete
    );
    let crlf = "// comment\r\nclass €Type {}";
    let crlf_tree = super::super::parse_tree(language, crlf).unwrap();
    let crlf_namespace = crate::code::java_namespace::collect(crlf_tree.root_node(), crlf).evidence;
    assert!(crlf_namespace.complete);
    assert_eq!(crlf_namespace.top_level_types, ["€Type"]);
    for name in ["€uro", "£ound", "‿name", "e\u{301}", "\u{10400}name"] {
        let source = format!(
            "package demo.{name}; class {name} {{ boolean run() {{ return Boolean.getBoolean(\"unicode.flag\"); }} }}"
        );
        let language = crate::code::languages::detect_language("App.java").unwrap();
        let tree = super::super::parse_tree(language, &source).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{name}: {}",
            tree.root_node().to_sexp()
        );
        let view = identifier_view(&source);
        assert_eq!(view.len(), source.len());
        let class = tree.root_node().named_child(1).unwrap();
        let node = class.child_by_field_name("name").unwrap();
        assert_eq!(&source[node.byte_range()], name);
        let namespace = crate::code::java_namespace::collect(tree.root_node(), &source).evidence;
        assert!(namespace.complete);
        assert_eq!(namespace.package, format!("demo.{name}"));
        assert_eq!(namespace.top_level_types, [name]);
    }
    for source in [
        "package demo.€uro; class App { boolean run( { return ; }",
        "package demo.\u{301}name; class App {}",
        "package demo.😀name; class App {}",
    ] {
        let language = crate::code::languages::detect_language("App.java").unwrap();
        assert!(
            super::super::parse_tree(language, source)
                .unwrap()
                .root_node()
                .has_error(),
            "{source}"
        );
    }
}

#[test]
fn java_identifier_view_borrows_unchanged_literals_comments_and_text_blocks() {
    for source in [
        "class App { String x = \"€ \\\" £\"; char y = '€'; } // £\n/* € */",
        "class App { String x = \"\"\"\n € \" £ \\\" ‿\n\"\"\"; }",
        "class App { String x = \"unterminated €",
        "class App { /* unterminated €",
        "class App {}",
    ] {
        let view = identifier_view(source);
        assert!(matches!(view, Cow::Borrowed(_)), "{source}");
        assert_eq!(view.as_ref(), source.as_bytes());
    }
}
