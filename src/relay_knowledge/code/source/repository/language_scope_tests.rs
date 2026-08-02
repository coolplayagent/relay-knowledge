use super::*;

#[test]
fn language_scope_handles_header_compatibility_and_document_fallback() {
    assert!(source_language_filter_allows(
        "include/api.h",
        &["cpp".to_owned()]
    ));
    assert!(source_language_filter_allows(
        "docs/operations.md",
        &["unknown".to_owned()]
    ));
    assert!(!source_language_filter_allows(
        "src/api.c",
        &["cpp".to_owned()]
    ));
}
