use super::*;
#[test]
fn spaced_dotenv_assignments_preserve_values_comments_and_source_spans() {
    let source = "# @config domain=payments hot-reload=true\nFLAG = true\nexport\tTEXT = 'a # b'\nMULTI = \"line1\nline2\" # comment\nEMPTY =\nDYNAMIC = ${OTHER}\nnot valid = false\n";
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

#[test]
fn dotenv_hashes_require_comment_boundaries_and_support_all_line_endings() {
    for newline in ["\n", "\r", "\r\n"] {
        let source = [
            "URL=https://example.test/#fragment",
            "TOKEN=abc#123",
            "FLAG=true # comment",
            "EMPTY=",
            "LAST=false",
        ]
        .join(newline)
            + newline;
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
        assert_eq!(rows.len(), 5);
        for (row, expected) in rows.iter().zip([
            "https://example.test/#fragment",
            "abc#123",
            "true",
            "",
            "false",
        ]) {
            assert_eq!(row.metadata.default_value.as_deref(), Some(expected));
            assert!(
                source[row.byte_range.start as usize..row.byte_range.end as usize]
                    .contains(&row.source_key)
            );
        }
    }
}

#[test]
fn dotenv_bom_is_ignored_only_at_the_file_start() {
    let source = "\u{feff}export\tFIRST=true\n\u{feff}SECOND=false\nTHIRD=true\n";
    let rows = extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: ".env",
        language_id: "unknown",
        content: source,
        config_facts: &[],
    })
    .unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>(),
        ["FIRST", "THIRD"]
    );
    assert_eq!(rows[0].byte_range.start, 0);
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("true"));
    assert!(source[rows[1].byte_range.start as usize..].starts_with("THIRD"));
}
