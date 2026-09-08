//! Feature-flag projection tests.

use crate::{code::SnapshotBuild, domain::CodeRepositoryRegistration};

use super::record_feature_flags;

#[test]
fn derives_boolean_configuration_facts_before_projection() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    record_feature_flags(
        &mut build,
        "config/flags.yaml",
        "flags-file",
        "yaml",
        "checkout_v2: true\n",
        None,
    )
    .expect("feature flags should project");

    assert!(build.feature_flags.iter().any(|record| {
        record.source_key == "checkout_v2" && record.edge_kind == "defines_config"
    }));
}

#[test]
fn syntax_projection_deduplicates_current_file_without_dropping_previous_file_facts() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .unwrap();
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        2,
        0,
    );
    super::super::parse_indexed_file(&mut build, "flags.properties", b"enabled=true\n").unwrap();
    super::super::parse_indexed_file(
        &mut build,
        "App.java",
        b"class App { String read() { return System.getenv(\"FLAG\"); } }\n",
    )
    .unwrap();
    assert!(
        build
            .feature_flags
            .iter()
            .any(|record| record.source_key == "enabled")
    );
    let environment = build
        .feature_flags
        .iter()
        .filter(|record| record.source_key == "FLAG")
        .collect::<Vec<_>>();
    assert_eq!(environment.len(), 1);
    assert_eq!(environment[0].excerpt, "System.getenv(\"FLAG\")");
    assert_eq!(
        environment[0].metadata.value_type.as_deref(),
        Some("string")
    );
}

#[test]
fn java_syntax_rejects_shadowed_receivers_and_keys_without_lexical_resurrection() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .unwrap();
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    super::super::parse_indexed_file(&mut build, "Shadow.java", br#"
class Shadow {
    static final String KEY = "actual_key";
    void parameter(String KEY) { System.getProperty(KEY); }
    void local() { String KEY = "dynamic"; System.getProperty(KEY); }
    void receiver() { Fake System = new Fake(); System.getProperty("fake_property"); System.getenv("FAKE_ENV"); }
    void qualified(Fake Shadow) { System.getProperty(Shadow.KEY); }
    void lambda() { java.util.function.Function<String, String> f = KEY -> System.getProperty(KEY); }
    void scoped(java.util.List<String> items) {
        java.util.function.Function<String, String> typed = (String KEY) -> System.getProperty(KEY);
        java.util.function.Function<String, String> inferred = (KEY) -> System.getProperty(KEY);
        for (String KEY : items) { System.getProperty(KEY); }
        try {} catch (FakeException System) { System.getProperty("fake_property"); }
        try (FakeResource System = new FakeResource()) { System.getenv("FAKE_ENV"); }
        System.getenv("AFTER_SCOPE");
    }
    void valid() {
        { String KEY = "dynamic"; Fake System = new Fake(); System.getenv("HIDDEN_ENV"); }
        System.getProperty(KEY); System.getenv("VISIBLE_ENV");
    }
}
class FieldShadow { Fake System; void read() { System.getenv("FIELD_ENV"); } }
"#).unwrap();
    assert!(!build.feature_flags.iter().any(|flag| matches!(
        flag.source_key.as_str(),
        "fake_property" | "FAKE_ENV" | "HIDDEN_ENV" | "FIELD_ENV"
    )));
    let reads = build
        .feature_flags
        .iter()
        .filter(|flag| flag.edge_kind == "reads_config")
        .collect::<Vec<_>>();
    assert_eq!(reads.len(), 3, "{reads:#?}");
    assert!(reads.iter().any(|flag| flag.source_key == "AFTER_SCOPE"));
    assert!(reads.iter().any(|flag| flag.source_key == "Shadow.KEY"));
    assert!(reads.iter().any(|flag| flag.source_key == "VISIBLE_ENV"));
}
