//! Direct XML structure, text, and source-line invariants.

use super::*;

#[test]
fn xml_parser_preserves_children_text_cdata_and_start_lines() {
    let root = parse_xml_document(
        "<project>\n  <artifactId>demo</artifactId>\n  <description><![CDATA[a < b]]></description>\n  <empty/>\n</project>",
    )
    .expect("XML should parse")
    .expect("root should exist");

    assert_eq!(root.name, "project");
    assert_eq!(root.line, 1);
    assert_eq!(root.child("artifactId").expect("artifact").text, "demo");
    assert_eq!(root.child("artifactId").expect("artifact").line, 2);
    assert_eq!(
        root.child("description").expect("description").text,
        "a < b"
    );
    assert_eq!(root.children_named("empty").count(), 1);
    assert_eq!(root.children().len(), 3);
}

#[test]
fn xml_parser_rejects_unclosed_documents() {
    assert!(parse_xml_document("<project><artifactId>demo</artifactId>").is_err());
}
