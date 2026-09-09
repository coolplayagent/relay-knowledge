use super::*;

#[test]
fn multiple_calls_in_one_action_preserve_distinct_read_occurrences_and_source_spans() {
    let content = "{{ printf `%s/%s` (key \"same\") (key \"same\") }}";
    let records = reads(content).unwrap();
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].usage_id, records[1].usage_id);
    for record in records {
        assert_eq!(record.byte_range.start, 0);
        assert_eq!(
            usize::try_from(record.byte_range.end).unwrap(),
            content.len()
        );
        assert_eq!(record.source_key, "same");
    }
}

fn reads(content: &str) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut records = Vec::new();
    collect(
        &FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "settings.ctmpl",
            language_id: "text",
            content,
            config_facts: &[],
        },
        &mut records,
    )?;
    Ok(records)
}

#[test]
fn multiline_template_reads_preserve_exact_utf8_crlf_ranges_and_default_values() {
    let content = "前缀\r\n  {{- key\r\n \"feature_x\" -}} {{ env\n `ENV_SWITCH` }}\n{{ keyOrDefault\n \"feature_y\"\n \"true\" }}";
    let records = reads(content).unwrap();
    assert_eq!(records.len(), 3);
    let first = &records[0];
    assert_eq!(first.source_key, "feature_x");
    assert_eq!(
        usize::try_from(first.byte_range.start).unwrap(),
        content.find("{{-").unwrap()
    );
    assert_eq!(
        usize::try_from(first.byte_range.end).unwrap(),
        content.find("-}}").unwrap() + 3
    );
    assert_eq!((first.line_range.start, first.line_range.end), (2, 3));
    assert_eq!(records[1].source_kind, "env_var");
    assert_eq!(
        (records[1].line_range.start, records[1].line_range.end),
        (3, 4)
    );
    assert_eq!(records[2].metadata.default_value.as_deref(), Some("true"));
    assert_eq!(records[2].metadata.value_type.as_deref(), Some("boolean"));
    assert_eq!(
        (records[2].line_range.start, records[2].line_range.end),
        (5, 7)
    );
}

#[test]
fn template_comments_and_quoted_delimiters_do_not_split_or_create_reads() {
    let content = "{{/* ignored {{ key \"fake\" }}\n */}}{{ keyOrDefault\n \"quoted\" \"a}}b\\\"c\" }}{{ keyOrDefault `raw` `x}}y` }}{{ key \"same\" }}{{ key \"same\" }}";
    let records = reads(content).unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records[0].metadata.default_value.as_deref(),
        Some("a}}b\"c")
    );
    assert_eq!(records[1].metadata.default_value.as_deref(), Some("x}}y"));
    assert_ne!(records[2].usage_id, records[3].usage_id);
    assert_eq!(records[2].source_key, "same");
    assert!(!records.iter().any(|r| r.source_key == "fake"));
}

#[test]
fn malformed_actions_and_dynamic_arguments_do_not_invent_static_reads_or_defaults() {
    assert!(
        reads("{{ key dynamic }} {{ key \"unterminated")
            .unwrap()
            .is_empty()
    );
    assert!(reads("{{/* key \"comment\" }}").unwrap().is_empty());
    assert!(reads("{{ key `unterminated }}").unwrap().is_empty());
    assert!(reads("{{ key \"bad/key\" }}").unwrap().is_empty());
    let records = reads("{{ keyOrDefault \"dynamic\" (env \"OTHER\") }}").unwrap();
    assert_eq!(records.len(), 2);
    assert!(records[0].metadata.default_value.is_none());
    assert_eq!(
        super::super::template_literals::string("`raw literal` rest"),
        Some(("raw literal".to_owned(), " rest"))
    );
    assert!(super::super::template_literals::string("\"unterminated").is_none());
}

#[test]
fn oversized_action_decoding_fails_explicitly_at_the_bound() {
    let action = format!("{{{{ key \"flag\" {} }}}}", " ".repeat(MAX_ACTION_BYTES));
    assert!(
        reads(&action)
            .unwrap_err()
            .to_string()
            .contains("65536-byte")
    );
    let prefix = "{{ key \"flag\" ";
    let exact = [
        prefix,
        &" ".repeat(MAX_ACTION_BYTES - prefix.len() - 2),
        "}}",
    ]
    .concat();
    assert_eq!(exact.len(), MAX_ACTION_BYTES);
    assert_eq!(reads(&exact).unwrap().len(), 1);
}
