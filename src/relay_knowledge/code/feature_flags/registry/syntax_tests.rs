//! Configuration syntax and exact Java receiver review regressions.
use super::*;
#[test]
fn java_super_and_constructed_receivers_preserve_exact_owner_evidence() {
    let rows = facts(
        "java",
        r#"package app; class Base { boolean isX() { return Boolean.getBoolean("base"); } }
      class Child extends Base { boolean isX() { return Boolean.getBoolean("child"); } void run() { if(super.isX()) {} if(new Base().isX()) {} if(((Base)new Child()).isX()) {} } }"#,
    );
    for owner in ["app.Base.isX", "app.Child.isX"] {
        assert!(
            rows.iter().any(|r| r.edge_kind == "guards_code"
                && r.metadata.reference.as_deref() == Some(owner)
                && r.metadata.exact_reference),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|r| r.metadata.declared_getter.as_deref() == Some(owner))
        );
    }
}
#[test]
fn sdk_evaluations_reuse_adjacent_annotation_metadata() {
    let content = "class App { void run() {\nvar client = OpenFeature.getClient();\n// @config domain=payments hot-reload=true\nclient.getBooleanValue(\"checkout\", false);\n// @config domain=ignored\n\nclient.getBooleanValue(\"plain\", false);\n}}";
    let rows = crate::code::feature_flags::extract_feature_flags(FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path: "App.java",
        language_id: "java",
        content,
        config_facts: &[],
    })
    .unwrap();
    let checkout = rows.iter().find(|r| r.source_key == "checkout").unwrap();
    assert_eq!(checkout.metadata.domain.as_deref(), Some("payments"));
    assert_eq!(checkout.metadata.hot_reload, Some(true));
    assert!(
        rows.iter()
            .find(|r| r.source_key == "plain")
            .unwrap()
            .metadata
            .domain
            .is_none()
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
fn java_numeric_defaults_are_canonical_values() {
    let rows = facts(
        "java",
        r#"class App { void run() { config.get("limit", 1_000); config.get("timeout", 10L); config.get("ratio", 10.0D); config.get("port." + 1_0L); config.get("float." + 1.0); }}"#,
    );
    for (key, expected) in [("limit", "1000"), ("timeout", "10"), ("ratio", "10")] {
        assert_eq!(
            rows.iter()
                .find(|r| r.source_key == key)
                .unwrap()
                .metadata
                .default_value
                .as_deref(),
            Some(expected)
        );
    }
    assert!(rows.iter().any(|r| r.source_key == "port.10"));
    assert!(!rows.iter().any(|r| r.source_key.starts_with("float.")));
}
