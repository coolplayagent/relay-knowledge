use super::*;
#[test]
fn spaced_dotenv_assignments_preserve_values_comments_and_source_spans() {
    let source = "# @config domain=payments hot-reload=true\nFLAG = true\nexport TEXT = 'a # b'\nMULTI = \"line1\nline2\" # comment\nEMPTY =\nDYNAMIC = ${OTHER}\nnot valid = false\n";
    let rows = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: ".env.example",
        language_id: "unknown",
        content: source,
        config_facts: &[],
    })
    .unwrap();
    assert_eq!(rows.len(), 5);
    for (key, value) in [
        ("FLAG", "true"),
        ("TEXT", "a # b"),
        ("MULTI", "line1\nline2"),
        ("EMPTY", ""),
    ] {
        let row = rows.iter().find(|r| r.source_key == key).unwrap();
        assert_eq!(row.metadata.default_value.as_deref(), Some(value));
        assert!(source[row.byte_range.start as usize..row.byte_range.end as usize].contains(key));
    }
    assert_eq!(rows[0].metadata.domain.as_deref(), Some("payments"));
    assert!(rows[4].metadata.flow_incomplete.is_some());
}
#[test]
fn oversized_dotenv_values_keep_definitions_without_blocking_following_keys() {
    let source = format!("CERT = '{}'\nSMALL = true\n", "x".repeat(70000));
    let rows = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: ".env",
        language_id: "unknown",
        content: &source,
        config_facts: &[],
    })
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].metadata.default_value.is_none());
    assert!(rows[0].metadata.flow_incomplete.is_some());
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("true"));
}
