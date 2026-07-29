use super::*;

#[test]
fn trims_and_preserves_source_scope() {
    let scope = SourceScope::parse(" docs/specs ").expect("scope should parse");

    assert_eq!(scope.as_str(), "docs/specs");
}

#[test]
fn rejects_empty_source_scope() {
    let error = SourceScope::parse(" ").expect_err("empty scope should fail");

    assert_eq!(error.field, "source_scope");
}

#[test]
fn rejects_nul_bytes_and_converts_to_string() {
    let error = SourceScope::parse("repo\0branch").expect_err("NUL should fail");
    let scope: String = SourceScope::parse("repo")
        .expect("scope should parse")
        .into();

    assert_eq!(error.field, "source_scope");
    assert_eq!(scope, "repo");
}
