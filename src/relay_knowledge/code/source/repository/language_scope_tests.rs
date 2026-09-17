use super::*;

#[test]
fn compound_language_scopes_preserve_manifest_and_document_paths() {
    for (left, right, admitted, excluded) in [
        ("rust", "toml", "Cargo.toml", "src/lib.rs"),
        ("javascript", "typescript", "package.json", "src/app.js"),
        ("java", "kotlin", "pom.xml", "src/Main.java"),
        ("unknown", "json", "docs/data.json", "src/lib.rs"),
        ("c", "cpp", "include/api.h", "src/main.cpp"),
    ] {
        let filters = crate::domain::code_scope_language_filters(&[left.into()], &[right.into()]);
        assert!(
            source_language_filter_allows(admitted, &filters),
            "{admitted}: {filters:?}"
        );
        assert!(
            !source_language_filter_allows(excluded, &filters),
            "{excluded}: {filters:?}"
        );
    }
}

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
