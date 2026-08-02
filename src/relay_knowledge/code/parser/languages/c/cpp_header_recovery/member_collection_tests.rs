use super::collect_class_member_declarations;
use crate::code::parser::languages::c::cpp_header_recovery::{
    source_text::source_lines, top_level_scan::top_level_body_open_start,
};

fn collect_members(
    source: &str,
) -> Vec<(
    String,
    Option<String>,
    &'static str,
    crate::code::parser::nodes::SyntaxRange,
)> {
    let lines = source_lines(source);
    let body_start =
        top_level_body_open_start(&lines[0].code).expect("test class should open a body");
    let mut definitions = Vec::new();
    collect_class_member_declarations(
        &lines,
        0,
        body_start + "{".len(),
        Some("Outer".to_owned()),
        &mut definitions,
    );
    definitions
}

#[test]
fn member_collection_traverses_nested_classes_and_skips_function_bodies() {
    let definitions = collect_members(
        "class Outer { public: void First(); class Inner { void Nested(); }; \
         void Inline() { if (ready) { run(); } } void Last(); };",
    );
    let names = definitions
        .iter()
        .map(|(name, qualified, _, _)| (name.as_str(), qualified.as_deref()))
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            ("First", Some("Outer.First")),
            ("Nested", Some("Outer.Inner.Nested")),
            ("Last", Some("Outer.Last")),
        ]
    );
}

#[test]
fn member_collection_materializes_documented_multiline_source_range() {
    let source = "class Outer {\n  // Opens the store.\n  Result Open(\n      int flags);\n};";
    let definitions = collect_members(source);
    let (_, qualified, kind, range) = definitions
        .iter()
        .find(|(name, _, _, _)| name == "Open")
        .expect("multiline member should be collected");

    assert_eq!(qualified.as_deref(), Some("Outer.Open"));
    assert_eq!(*kind, "function_declaration");
    assert_eq!((range.line_start, range.line_end), (2, 4));
    assert_eq!(
        &source[range.byte_start..range.byte_end],
        "  // Opens the store.\n  Result Open(\n      int flags);"
    );
}
