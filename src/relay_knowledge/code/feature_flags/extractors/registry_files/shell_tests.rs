use super::*;

#[test]
fn exported_definitions_decode_static_quote_concatenation_once() {
    for (source, expected, kind) in [
        (r#"export FLAG=tr"u"'e'"#, "true", "boolean"),
        (r#"export FLAG="1"'2'"#, "12", "integer"),
        ("export FLAG=''", "", "string"),
        (r#"export FLAG="'quoted'""#, "'quoted'", "string"),
    ] {
        let records = facts(source);
        let record = records
            .iter()
            .find(|r| r.edge_kind == "defines_config")
            .unwrap();
        assert_eq!(record.metadata.default_value.as_deref(), Some(expected));
        assert_eq!(record.metadata.value_type.as_deref(), Some(kind));
    }
    for source in [r#"export FLAG="$OTHER""#, r#"export FLAG="\n""#] {
        let records = facts(source);
        assert!(records.iter().all(|r| r.metadata.default_value.is_none()));
    }
}

#[test]
fn exported_declaration_definitions_share_the_binding_option_contract() {
    for command in [
        "export\tFLAG=true",
        "declare -x FLAG=true",
        "typeset -rx FLAG=true",
        "local -x FLAG=true",
    ] {
        let records = facts(&format!("{command}\necho $FLAG\n"));
        assert_eq!(
            records
                .iter()
                .filter(|record| record.edge_kind == "defines_config")
                .count(),
            1,
            "{command}: {records:?}"
        );
        assert_eq!(
            records
                .iter()
                .find(|record| record.edge_kind == "defines_config")
                .unwrap()
                .metadata
                .default_value
                .as_deref(),
            Some("true")
        );
    }
    for command in [
        "export -n FLAG=true",
        "declare +x FLAG=true",
        "local FLAG=true",
    ] {
        assert!(
            !facts(&format!("{command}\n"))
                .iter()
                .any(|record| record.edge_kind == "defines_config"),
            "{command}"
        );
    }
}

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

#[test]
fn all_standard_parameter_operators_keep_the_parameter_and_only_real_defaults() {
    for operator in ["-", ":-", "=", ":="] {
        let records = facts(&format!("echo \"${{FLAG{operator}value}}\"\n"));
        assert_eq!(records.len(), 1, "{operator}: {records:?}");
        assert_eq!(records[0].source_key, "FLAG");
        assert_eq!(records[0].metadata.default_value.as_deref(), Some("value"));
    }
    for expression in [
        "FLAG+enabled",
        "FLAG:+enabled",
        "FLAG?required",
        "FLAG:?required",
        "#FLAG",
        "FLAG#pat",
        "FLAG##pat",
        "FLAG%pat",
        "FLAG%%pat",
        "FLAG/foo/bar",
        "FLAG//foo/bar",
        "FLAG:1:2",
        "FLAG^",
        "FLAG^^",
        "FLAG,",
        "FLAG,,",
        "FLAG@Q",
        "!FLAG",
    ] {
        let records = facts(&format!("echo \"${{{expression}}}\"\n"));
        assert_eq!(records.len(), 1, "{expression}: {records:?}");
        assert_eq!(records[0].source_key, "FLAG", "{expression}");
        assert!(records[0].metadata.default_value.is_none(), "{expression}");
    }
    let records = facts("echo \"${FLAG:-$OTHER}\"\n");
    assert!(
        records
            .iter()
            .find(|r| r.source_key == "FLAG")
            .unwrap()
            .metadata
            .default_value
            .is_none()
    );
}

#[test]
fn excludes_unexported_local_data_flow_and_preserves_external_and_exported_reads() {
    let records = facts(
        "LOCAL=false\necho $LOCAL\nf() { local INNER=false; echo ${INNER:-true}; }\nexport GOOD=true\necho $GOOD\necho ${EXTERNAL:-false}\n",
    );
    assert!(
        !records
            .iter()
            .any(|record| matches!(record.source_key.as_str(), "LOCAL" | "INNER"))
    );
    assert!(
        records
            .iter()
            .any(|record| record.source_key == "GOOD" && record.edge_kind == "reads_config")
    );
    assert!(records.iter().any(|record| record.source_key == "EXTERNAL"
        && record.metadata.default_value.as_deref() == Some("false")));
}

#[test]
fn static_shell_fallback_quotes_follow_the_enclosing_parameter_context() {
    for (source, expected, kind) in [
        (r#"echo ${FLAG:-"true"}"#, "true", "boolean"),
        ("echo ${FLAG:-''}", "", "string"),
        (r#"echo "${FLAG:-''}""#, "''", "string"),
        (r#"echo ${FLAG:-tr"u"'e'}"#, "true", "boolean"),
        (r#"echo ${FLAG:="12"}"#, "12", "integer"),
        (r#"echo "${FLAG:-"true"}""#, "true", "boolean"),
    ] {
        let records = facts(source);
        let record = records
            .iter()
            .find(|record| record.source_key == "FLAG")
            .unwrap();
        assert_eq!(
            record.metadata.default_value.as_deref(),
            Some(expected),
            "{source}"
        );
        assert_eq!(
            record.metadata.value_type.as_deref(),
            Some(kind),
            "{source}"
        );
    }
    for source in [r#"echo ${FLAG:-"$OTHER"}"#, r#"echo ${FLAG:-$(command)}"#] {
        let records = facts(source);
        let record = records
            .iter()
            .find(|record| record.source_key == "FLAG")
            .unwrap();
        assert!(record.metadata.default_value.is_none());
    }
}
#[test]
fn unsupported_or_oversized_static_operands_remain_unknown() {
    assert!(static_fallback("'unterminated", false).is_none());
    assert!(static_fallback(&"x".repeat(65_537), false).is_none());
    assert_eq!(static_fallback("", false).as_deref(), Some(""));
}
