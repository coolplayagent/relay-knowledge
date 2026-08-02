use super::{boolean_config_keys, facts};
use crate::code::config_files::model::ConfigValueKind;

#[test]
fn identifies_unquoted_boolean_keys_without_duplicate_inline_matches() {
    assert_eq!(boolean_config_keys("yaml", "enabled: true"), ["enabled"]);
    assert!(boolean_config_keys("yaml", "enabled: \"true\"").is_empty());
    assert_eq!(
        boolean_config_keys("properties", "feature.enabled disabled"),
        ["feature.enabled"]
    );
}

#[test]
fn preserves_boolean_value_kind_in_extracted_facts() {
    let mut definitions = Vec::new();

    facts(
        "yaml",
        "enabled: true\nlabel: stable\nquoted: \"false\"\n",
        &mut definitions,
    );

    let enabled = definitions
        .iter()
        .find(|fact| fact.name == "enabled")
        .expect("enabled fact");
    assert_eq!(enabled.value_kind, ConfigValueKind::Boolean);
    assert_eq!(
        definitions
            .iter()
            .find(|fact| fact.name == "quoted")
            .map(|fact| fact.value_kind),
        Some(ConfigValueKind::Unknown)
    );
}
