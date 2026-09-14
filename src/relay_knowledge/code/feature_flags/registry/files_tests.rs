use crate::code::feature_flags::registry::test_support::*;

#[test]
fn template_output_and_ini_bare_lines_are_not_configuration_definitions() {
    for language in ["ini", "gotemplate"] {
        let rows = facts(language, "server {\n}\nbare\nvalid = true\nother: false\n");
        let keys = rows
            .iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["valid", "other"]);
    }
    let rows = facts("properties", "bare\nspace value\n");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some(""));
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("value"));
}

#[test]
fn template_key_or_default_retains_literal_fallback_and_type() {
    let rows = facts(
        "gotemplate",
        r#"{{ keyOrDefault "feature_x" "off" }}
        {{ keyOrDefault "enabled" "true" }}
        {{ keyOrDefault "dynamic" $fallback }}
        {{ key "without" }}"#,
    );
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("off"));
    assert_eq!(rows[0].metadata.value_type.as_deref(), Some("string"));
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("true"));
    assert_eq!(rows[1].metadata.value_type.as_deref(), Some("boolean"));
    assert!(rows[2..].iter().all(|r| r.metadata.default_value.is_none()));
}

#[test]
fn properties_defaults_keep_trailing_whitespace_and_continuations() {
    let rows = facts("properties", "mode=on \ncontinued=on\\\n  value \t\n");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on "));
    assert_eq!(
        rows[1].metadata.default_value.as_deref(),
        Some("onvalue \t")
    );
}

#[test]
fn configuration_ranges_exclude_terminal_newlines() {
    for newline in ["\n", "\r\n"] {
        for language in ["properties", "ini", "gotemplate"] {
            let single = facts(language, &format!("feature=true{newline}"));
            assert_eq!(single[0].line_range.start, 1);
            assert_eq!(single[0].line_range.end, 1);
            assert_eq!(single[0].byte_range.end, 12);
        }
        let continued = facts("properties", &format!("feature=tr\\{newline}ue{newline}"));
        assert_eq!(continued[0].line_range.end, 2);
    }
}

#[test]
fn templates_separate_declarations_from_reads_and_keep_quoted_delimiters() {
    let rows = facts(
        "gotemplate",
        "feature_x={{ key \"feature_x\" }}\n{{ env \"FLAG\" }}\nfeature_y={{ key \"a}}b\" }}\n",
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r.edge_kind == "declares_config_key")
            .count(),
        2
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "a}}b" && r.edge_kind == "reads_config")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "env_var" && r.source_key == "FLAG")
    );
    assert!(!rows.iter().any(|r| r.edge_kind == "defines_config"));
}

#[test]
fn ini_sections_do_not_alias_same_named_keys() {
    let rows = facts("ini", "feature_x=true\n[service]\nfeature_x=false\n");
    assert_eq!(
        rows.iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>(),
        ["feature_x", "service.feature_x"]
    );
}

#[test]
fn properties_escapes_do_not_corrupt_ini_or_template_defaults() {
    for language in ["ini", "gotemplate"] {
        let rows = facts(language, r"directory=C:\temp\files");
        assert_eq!(
            rows[0].metadata.default_value.as_deref(),
            Some(r"C:\temp\files")
        );
    }
    assert_eq!(
        facts("properties", r"directory=C:\temp")[0]
            .metadata
            .default_value
            .as_deref(),
        Some("C:\temp")
    );
}

#[test]
fn properties_semicolons_are_key_content_while_ini_semicolons_are_comments() {
    let rows = facts("properties", ";feature.enabled=true\n");
    assert_eq!(rows[0].source_key, ";feature.enabled");
    assert!(facts("ini", ";feature.enabled=true\n").is_empty());
}

#[test]
fn properties_escaped_trailing_key_spaces_survive_assignment_splitting() {
    let rows = facts("properties", "feature\\ =on\nbare\\ \n");
    assert_eq!(rows[0].source_key, "feature ");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on"));
    assert_eq!(rows[1].source_key, "bare ");
}

#[test]
fn template_control_pipelines_extract_nested_calls_without_reading_quoted_text() {
    let rows = facts(
        "gotemplate",
        r#"{{ with key "feature_x" }}
        {{ if (keyOrDefault "enabled" "false") }}
        {{ if eq (env "FLAG") "yes" }}
        {{ printf "key \"not_a_read\"" }}
        {{ if and (key "same") (key "same") }}"#,
    );
    assert_eq!(rows.len(), 5);
    assert!(
        rows.iter()
            .any(|r| r.source_key == "enabled"
                && r.metadata.default_value.as_deref() == Some("false"))
    );
    assert!(
        rows.iter()
            .any(|r| r.source_kind == "env_var" && r.source_key == "FLAG")
    );
    let repeated = rows
        .iter()
        .filter(|r| r.source_key == "same")
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_ne!(repeated[0].usage_id, repeated[1].usage_id);
}

#[test]
fn properties_flush_pending_continuations_at_eof() {
    for source in ["flag=on\\", "flag=on\\\n", "flag=\\\n  on\\"] {
        let rows = facts("properties", source);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].metadata.default_value.as_deref(), Some("on"));
    }
}

#[test]
fn properties_natural_lines_preserve_offsets_and_continuations() {
    for ending in ["\r", "\n", "\r\n"] {
        let content =
            format!("flag=true{ending}other=false{ending}long=first\\{ending} second{ending}");
        let rows = facts("properties", &content);
        assert_eq!(rows.len(), 3, "{ending:?}");
        assert_eq!(rows[1].source_key, "other");
        assert_eq!(rows[1].line_range.start, 2);
        assert_eq!(rows[1].line_range.end, 2);
        assert_eq!(
            &content[rows[1].byte_range.start as usize..rows[1].byte_range.end as usize],
            "other=false"
        );
        assert_eq!(
            rows[2].metadata.default_value.as_deref(),
            Some("firstsecond")
        );
        assert_eq!(rows[2].line_range.start, 3);
        assert_eq!(rows[2].line_range.end, 4);
    }
}

#[test]
fn properties_comments_cannot_continue_into_definitions() {
    for comment in ["# note\\", "! note\\", "  # note\\"] {
        let rows = facts("properties", &format!("{comment}\nflag=true\n"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_key, "flag");
        assert_eq!(rows[0].line_range.start, 2);
    }
}

#[test]
fn template_keys_and_defaults_follow_go_escape_semantics() {
    let rows = facts(
        "gotemplate",
        r#"{{ key "feature\x2ex" }} {{ keyOrDefault "\U00000061" "\141" }}"#,
    );
    assert_eq!(rows[0].source_key, "feature.x");
    assert_eq!(rows[1].source_key, "a");
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("a"));
}

#[test]
fn template_comment_quotes_do_not_consume_following_actions() {
    for comment in [
        r#"{{/* document "quoted value */}}"#,
        r#"{{- /* ` unclosed " */ -}}"#,
    ] {
        let rows = facts("gotemplate", &format!("{comment}\n{{{{ key \"flag\" }}}}"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_key, "flag");
    }
}

#[test]
fn properties_non_java_whitespace_remains_key_and_value_content() {
    let rows = facts(
        "properties",
        "\u{a0}feature\u{a0}name=\u{a0}value\n\tother\u{c}=\ttrue\n",
    );
    assert_eq!(rows[0].source_key, "\u{a0}feature\u{a0}name");
    assert_eq!(
        rows[0].metadata.default_value.as_deref(),
        Some("\u{a0}value")
    );
    assert_eq!(rows[1].source_key, "other");
    assert_eq!(rows[1].metadata.default_value.as_deref(), Some("true"));
    let rows = facts("properties", "\u{a0}# @config domain=fake\nflag=true\n");
    assert!(rows.iter().all(|row| row.metadata.domain.is_none()));
}

#[test]
fn ini_and_template_backslashes_do_not_escape_separators() {
    for language in ["ini", "gotemplate"] {
        let rows = facts(language, "feature\\=true");
        assert_eq!(rows[0].source_key, "feature\\");
        assert_eq!(rows[0].metadata.default_value.as_deref(), Some("true"));
    }
    assert_eq!(
        facts("properties", "feature\\=true")[0].source_key,
        "feature=true"
    );
}

#[test]
fn ini_exclamation_keys_are_definitions_while_properties_uses_comments() {
    let rows = facts(
        "ini",
        "!important=true\n[section]\n!enabled=false\n; ignored=true\n# ignored=true\n",
    );
    assert!(rows.iter().any(|r| r.source_key == "!important"));
    assert!(rows.iter().any(|r| r.source_key == "section.!enabled"));
    assert_eq!(rows.len(), 2);
    assert!(facts("properties", "!important=true").is_empty());
}

#[test]
fn template_niladic_arguments_are_not_reader_commands() {
    let rows = facts(
        "gotemplate",
        r#"{{ printf "%s %s" key "fake" }} {{ printf "%s" (key "real") }}
      {{ if key "condition" }}{{ end }} {{ $x := key "assigned" }} {{ "value" | key "piped" }}
      {{ printf "%s" (env "HOST") keyOrDefault "also_fake" "false" }}"#,
    );
    let keys = rows
        .iter()
        .map(|r| r.source_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["real", "condition", "assigned", "piped", "HOST"]);
}

#[test]
fn template_pipelines_pass_literals_as_final_reader_arguments() {
    let rows = facts(
        "gotemplate",
        r#"{{ "consul/feature" | key }} {{ `HOST` | env }} {{ "false" | keyOrDefault "feature" }} {{ if "condition" | key }}{{end}} {{ printf "not_a_key" | key }}"#,
    );
    let keys = rows
        .iter()
        .map(|r| r.source_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["consul/feature", "HOST", "feature", "condition"]);
    let fallback = rows.iter().find(|r| r.source_key == "feature").unwrap();
    assert_eq!(fallback.metadata.default_value.as_deref(), Some("false"));
    assert!(fallback.excerpt.contains("keyOrDefault"));
    assert!(rows.iter().all(|r| r.byte_range.start < r.byte_range.end));
}

#[test]
fn template_grouped_literals_are_reader_arguments() {
    let rows = facts(
        "gotemplate",
        r#"{{ key ("consul/feature") }} {{ env (("HOST")) }} {{ keyOrDefault ("feature") (("false")) }} {{ key (printf "not_static") }}"#,
    );
    assert_eq!(
        rows.iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>(),
        ["consul/feature", "HOST", "feature"]
    );
    assert_eq!(rows[2].metadata.default_value.as_deref(), Some("false"));
}

#[test]
fn template_comments_preserve_adjacent_metadata_and_following_output() {
    let rows = facts(
        "gotemplate",
        "{{/*\n@config domain=payments hot-reload=true\n*/}}\n{{ key \"feature\" }}\n{{/* generated */}}feature=true\n{{- /* generated */ -}}other=false\n",
    );
    let read = rows.iter().find(|r| r.edge_kind == "reads_config").unwrap();
    assert_eq!(read.metadata.domain.as_deref(), Some("payments"));
    assert_eq!(read.metadata.hot_reload, Some(true));
    assert!(
        rows.iter()
            .all(|r| matches!(r.source_key.as_str(), "feature" | "other"))
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "feature" && r.edge_kind == "defines_config")
    );
    assert!(
        rows.iter().any(
            |r| r.source_key == "other" && r.metadata.default_value.as_deref() == Some("false")
        )
    );
}

#[test]
fn multiline_template_comments_hide_assignments_and_preserve_trailing_output() {
    let rows = facts(
        "gotemplate",
        "{{/* generated\nhidden=false\n*/}}visible=true\n",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source_key, "visible");
    assert_eq!(rows[0].metadata.default_value.as_deref(), Some("true"));
}

#[test]
fn oversized_configuration_defaults_retain_bounded_incomplete_facts() {
    for language in ["properties", "ini", "gotemplate"] {
        for value in ["x".repeat(70000), "x\t".repeat(22000)] {
            let rows = facts(language, &format!("large={value}end\nsmall=true\n"));
            let large = rows.iter().find(|r| r.source_key == "large").unwrap();
            assert!(large.metadata.default_value.is_none());
            assert!(large.metadata.flow_incomplete.is_some());
            assert!(serde_json::to_string(&large.metadata).unwrap().len() < 65536);
            assert!(
                rows.iter().any(|r| r.source_key == "small"
                    && r.metadata.default_value.as_deref() == Some("true"))
            );
        }
    }
}

#[test]
fn template_fallbacks_obey_decoded_and_serialized_value_budgets() {
    for value in [
        format!("`{}`", "x".repeat(70000)),
        format!("\"{}\"", "\\t".repeat(40000)),
        format!("\"{}\"", "\\a".repeat(12000)),
    ] {
        let rows = facts(
            "gotemplate",
            &format!("{{{{ keyOrDefault \"feature\" {value} }}}}"),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].edge_kind, "reads_config");
        assert!(rows[0].metadata.default_value.is_none());
        assert!(rows[0].metadata.flow_incomplete.is_some());
        assert!(serde_json::to_string(&rows[0].metadata).unwrap().len() < 65536);
    }
}
#[test]
fn template_embedded_comments_preserve_static_default_output() {
    for source in [
        "feature=true{{/* generated */}}\n",
        "feature=tr{{/* generated */}}ue\n",
        "feature=tr{{/* generated\nhidden=false\n*/}}ue\n",
        "feature=true {{- /* generated */ -}}\n",
    ] {
        let rows = facts("gotemplate", source);
        assert_eq!(rows.len(), 1, "{source}");
        assert_eq!(rows[0].edge_kind, "defines_config");
        assert_eq!(
            rows[0].metadata.default_value.as_deref(),
            Some("true"),
            "{source}"
        );
    }
}

#[test]
fn grouped_pipeline_inputs_retain_keys_and_fallbacks() {
    let rows = facts(
        "gotemplate",
        r#"{{ ("feature") | key }} {{ (("HOST")) | env }} {{ (("false")) | keyOrDefault ("fallback") }} {{ printf ("not_key") | key }}"#,
    );
    assert_eq!(
        rows.iter()
            .map(|r| r.source_key.as_str())
            .collect::<Vec<_>>(),
        ["feature", "HOST", "fallback"]
    );
    assert_eq!(rows[2].metadata.default_value.as_deref(), Some("false"));
}

#[test]
fn multiline_template_actions_do_not_become_output_assignments() {
    let rows = facts(
        "gotemplate",
        "{{\n$value := key \"feature\"\n}}defined=true\nvalue={{\nkey \"other\"\n}}\n",
    );
    assert!(
        rows.iter().all(|r| !r.source_key.starts_with('$')),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "feature" && r.edge_kind == "reads_config")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "defined"
                && r.metadata.default_value.as_deref() == Some("true"))
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "value" && r.edge_kind == "declares_config_key")
    );
    assert!(
        rows.iter()
            .any(|r| r.source_key == "other" && r.edge_kind == "reads_config")
    );
}
