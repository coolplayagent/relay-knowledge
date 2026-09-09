use super::*;

#[test]
fn template_key_or_default_preserves_only_static_literal_fallbacks() {
    let records = facts(
        "config.ctmpl",
        "{{ keyOrDefault \"flag\" \"true\" }}\n{{ keyOrDefault \"label\" \"hello world\" }}\n{{ keyOrDefault \"dynamic\" (env \"OTHER\") }}\n{{ key \"plain\" }}\n",
    );
    let flag = records
        .iter()
        .find(|record| record.source_key == "flag")
        .unwrap();
    assert_eq!(flag.metadata.default_value.as_deref(), Some("true"));
    assert_eq!(flag.metadata.value_type.as_deref(), Some("boolean"));
    assert_eq!(
        records
            .iter()
            .find(|record| record.source_key == "label")
            .unwrap()
            .metadata
            .default_value
            .as_deref(),
        Some("hello world")
    );
    assert!(
        records
            .iter()
            .filter(|record| matches!(record.source_key.as_str(), "dynamic" | "plain"))
            .all(|record| record.metadata.default_value.is_none())
    );
}

fn facts(path: &str, content: &str) -> Vec<CodeFeatureFlagRecord> {
    let language_id = path.rsplit('.').next().unwrap_or_default();
    let config_facts = crate::code::config_files::structured_facts(path, language_id, content).0;
    extract(&FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path,
        language_id,
        content,
        config_facts: &config_facts,
    })
    .unwrap()
}

#[test]
fn extracts_scalar_metadata_and_only_explicit_domain_reload_declarations() {
    let records = facts(
        "config.properties",
        "# @config domain=business hot-reload=true\nfeature_x=true\ntimeout=12\nlabel=hello\n",
    );
    let records = records
        .iter()
        .fold(std::collections::BTreeMap::new(), |mut map, record| {
            map.insert(record.source_key.as_str(), record);
            map
        })
        .into_values()
        .collect::<Vec<_>>();
    let enabled = records
        .iter()
        .find(|r| r.source_key == "feature_x")
        .unwrap();
    let timeout = records.iter().find(|r| r.source_key == "timeout").unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(enabled.metadata.domain.as_deref(), Some("business"));
    assert_eq!(enabled.metadata.hot_reload, Some(true));
    assert_eq!(enabled.metadata.value_type.as_deref(), Some("boolean"));
    assert_eq!(timeout.metadata.default_value.as_deref(), Some("12"));
    assert_eq!(timeout.metadata.value_type.as_deref(), Some("integer"));
    assert!(timeout.metadata.domain.is_none());
    assert!(timeout.metadata.hot_reload.is_none());
    assert_eq!(records[2].metadata.source_format, "properties");
}

#[test]
fn extracts_ini_templates_and_shell_defaults_without_treating_expressions_as_values() {
    assert!(!facts("config.ini", "[business]\nfeature_x=false\n").is_empty());
    let template = facts("config.ctmpl", "feature_x={{ key \"feature_x\" }}\n");
    assert_eq!(template.len(), 2);
    assert!(
        template
            .iter()
            .all(|fact| fact.metadata.default_value.is_none())
    );
    assert!(template.iter().any(|fact| fact.edge_kind == "reads_config"));
    assert!(facts("readme.md", "feature_x=true").is_empty());
    assert!(facts("config.sh", "# export IGNORED=true\necho ordinary=text\n").is_empty());
}

#[test]
fn preserves_structured_properties_continuation_and_separator_rules() {
    let records = facts(
        "config.properties",
        "first: true\nsecond false\nmessage=long\\\n  fake=true\n",
    );
    assert!(records.iter().any(|r| r.source_key == "first"));
    assert!(records.iter().any(|r| r.source_key == "second"));
    assert!(!records.iter().any(|r| r.source_key == "fake"));
    assert!(
        records
            .iter()
            .filter(|r| r.source_key == "message")
            .all(|r| r.metadata.default_value.is_none())
    );
}

#[test]
fn preserves_properties_quote_characters_as_literal_value_data() {
    let records = facts("config.properties", "quoted=\"true\"\n");
    assert_eq!(
        records[0].metadata.default_value.as_deref(),
        Some("\"true\"")
    );
    assert_eq!(records[0].metadata.value_type.as_deref(), Some("string"));
}

#[test]
fn rejects_malformed_templates_and_invalid_metadata() {
    assert!(facts("config.ctmpl", "{{ key dynamic }}\n{{ key \"unterminated\n").is_empty());
    let records = facts(
        "config.ini",
        "# @config domain=bad/value hot-reload=maybe\nx=1.5\n",
    );
    assert!(records[0].metadata.domain.is_none());
    assert!(records[0].metadata.hot_reload.is_none());
    assert_eq!(records[0].metadata.value_type.as_deref(), Some("number"));
}

#[test]
fn detected_ini_aliases_and_annotation_adjacency_preserve_metadata_contract() {
    for path in ["application.conf", "settings.cfg", "settings.INI"] {
        let content = "key=hello\n";
        let structured = crate::code::config_files::structured_facts(path, "ini", content).0;
        let records = extract(&FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path,
            language_id: "ini",
            content,
            config_facts: &structured,
        })
        .unwrap();
        assert_eq!(records.len(), 1, "{path}");
        assert_eq!(records[0].metadata.source_format, "ini");
        assert_eq!(records[0].metadata.default_value.as_deref(), Some("hello"));
    }
    for separator in ["\n", "# unrelated\n", "; unrelated\n", "! unrelated\n"] {
        let records = facts(
            "config.properties",
            &format!("# @config domain=leaked hot-reload=true\n{separator}key=hello\n"),
        );
        assert!(records[0].metadata.domain.is_none());
        assert!(records[0].metadata.hot_reload.is_none());
    }
    let records = facts(
        "config.ini",
        "# @config domain=leaked hot-reload=true\n[section]\nkey=hello\n",
    );
    assert!(records[0].metadata.domain.is_none());
}
