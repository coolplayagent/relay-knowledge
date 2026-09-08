use super::*;

fn facts(content: &str) -> Vec<CodeFeatureFlagRecord> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(content, None).unwrap();
    extract(
        &FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "config.sh",
            language_id: "bash",
            content,
            config_facts: &[],
        },
        tree.root_node(),
    )
    .unwrap()
}

#[test]
fn extracts_export_and_read_defaults_from_bash_syntax() {
    let records = facts("export FEATURE_X=${FEATURE_X:-true}\n");
    assert_eq!(records.len(), 2);
    assert!(records.iter().any(|r| r.edge_kind == "defines_config"));
    assert!(
        records.iter().any(|r| r.edge_kind == "reads_config"
            && r.metadata.default_value.as_deref() == Some("true"))
    );
}

#[test]
fn excludes_comments_single_quotes_heredocs_and_nonliteral_defaults() {
    let records = facts(
        "# ${COMMENT}\necho '${QUOTED}'\ncat <<'EOF'\n${HEREDOC}\nEOF\nexport LIVE=${LIVE:-$OTHER}\n",
    );
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.source_key.as_str(), "COMMENT" | "QUOTED" | "HEREDOC"))
    );
    assert!(
        records
            .iter()
            .filter(|r| r.source_key == "LIVE")
            .all(|r| r.metadata.default_value.is_none())
    );
}
