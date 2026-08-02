use super::*;

#[test]
fn local_targets_normalize_wrappers_escapes_queries_and_percent_encoding() {
    assert_eq!(
        local_markdown_target("<docs/My%20Guide.md?view=full#intro>"),
        Some("docs/My Guide.md".to_owned())
    );
    assert_eq!(
        local_markdown_target(r"docs/name\#part.md"),
        Some("docs/name#part.md".to_owned())
    );
}

#[test]
fn local_targets_reject_fragments_protocol_relative_and_uri_schemes() {
    for target in [
        "",
        "#section",
        "//cdn.example.test/file.md",
        "https://example.test/file.md",
        "mailto:user@example.test",
    ] {
        assert_eq!(local_markdown_target(target), None);
    }
}
