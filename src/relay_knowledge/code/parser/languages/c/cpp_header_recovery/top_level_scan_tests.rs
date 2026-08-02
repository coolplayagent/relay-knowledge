//! C/C++ top-level scanning contract tests.

use super::{
    first_top_level_body_delimiter, identifier_spans_outside_groups, top_level_semicolon_positions,
};

#[test]
fn body_delimiters_ignore_literals_and_parameter_groups() {
    let code = r#"void Configure(const char *value = "}", int values[2]) { return; }"#;

    assert_eq!(
        first_top_level_body_delimiter(code),
        Some((code.find('{').expect("body opener"), '{'))
    );
}

#[test]
fn semicolon_scan_ignores_literals_and_nested_groups() {
    let code = r#"void Open(const char *value = ";"); int count;"#;
    let first = code.find(");").expect("first declaration terminator") + 1;

    assert_eq!(
        top_level_semicolon_positions(code),
        vec![first, code.rfind(';').expect("last declaration terminator")]
    );
}

#[test]
fn identifier_scan_omits_group_payloads_without_losing_declarator_tokens() {
    let code = "EXPORT_MACRO(hidden) alignas(8) Visible final";
    let identifiers = identifier_spans_outside_groups(code)
        .into_iter()
        .map(|(start, end)| &code[start..end])
        .collect::<Vec<_>>();

    assert_eq!(
        identifiers,
        vec!["EXPORT_MACRO", "alignas", "Visible", "final"]
    );
}
