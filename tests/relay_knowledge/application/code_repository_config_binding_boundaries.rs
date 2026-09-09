//! Real Git indexing and query coverage for configuration binding boundaries.
use super::*;

#[tokio::test]
async fn static_java_platform_imports_survive_git_indexing_without_lexical_shadow_facts() {
    let repo = FixtureRepo::create("config-static-imports");
    repo.write(
        "src/Explicit.java",
        r#"package demo;
import static java.lang.System.getenv;
import static java.lang.System.getProperty;
import static java.lang.Boolean.getBoolean;
class Explicit {
 void run(String getenv) {
  if (getenv("STATIC_ENV") != null) {}
  if (getProperty("static_property", "true") != null) {}
  if (getBoolean("static_boolean")) {}
 }
}
"#,
    );
    repo.write(
        "src/Wildcard.java",
        r#"package demo;
import static java.lang.System.*;
import static java.lang.Boolean.*;
class Wildcard {
 void run() {
  if (getenv("WILD_ENV") != null) {}
  if (getProperty("wild_property") != null) {}
  if (getBoolean("wild_boolean")) {}
 }
}
"#,
    );
    repo.write(
        "src/Shadow.java",
        r#"package demo;
import static java.lang.System.getenv;
class Shadow {
 static String getenv(String name) { return name; }
 void run() { if (getenv("false_local") != null) {} }
}
"#,
    );
    repo.write(
        "src/Custom.java",
        "package demo; class Custom { static String getenv(String name) { return name; } }\n",
    );
    repo.write(
        "src/CustomImport.java",
        r#"package demo;
import static java.lang.System.*;
import static demo.Custom.getenv;
class CustomImport { void run() { if (getenv("false_import") != null) {} } }
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Static platform imports and shadows"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-static-imports").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-static-imports"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-static-imports"),
        )
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 6, "{:?}", response.flags);
    for key in [
        "STATIC_ENV",
        "static_property",
        "static_boolean",
        "WILD_ENV",
        "wild_property",
        "wild_boolean",
    ] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        assert_eq!(
            flag.source_kind,
            if key.ends_with("ENV") {
                "env_var"
            } else {
                "config_key"
            }
        );
        assert_eq!(
            flag.usages
                .iter()
                .filter(|usage| usage.edge_kind == "reads_config")
                .count(),
            1,
            "{key}"
        );
        assert_eq!(
            flag.usages
                .iter()
                .filter(|usage| usage.edge_kind == "guards_code")
                .count(),
            1,
            "{key}"
        );
    }
    assert_eq!(
        response
            .flags
            .iter()
            .find(|flag| flag.source_key == "static_property")
            .unwrap()
            .usages
            .iter()
            .find(|usage| usage.edge_kind == "reads_config")
            .unwrap()
            .metadata
            .default_value
            .as_deref(),
        Some("true")
    );
}

#[tokio::test]
async fn getter_type_owners_guard_write_order_and_export_options_survive_git_indexing() {
    let repo = FixtureRepo::create("config-type-guard-export");
    repo.write("src/App.java", r#"package demo;
record RecordConfig() { String getValue() { return System.getProperty("record_key"); } }
enum EnumConfig { INSTANCE; String getValue() { return System.getProperty("enum_key"); } }
interface DefaultConfig { default String getValue() { return System.getProperty("interface_key"); } }
class App {
 void read(RecordConfig record, EnumConfig enumeration, DefaultConfig defaults) {
  if (record.getValue() != null) {}
  if (enumeration.getValue() != null) {}
  if (defaults.getValue() != null) {}
 }
 void branch() {
  boolean enabled = Boolean.getBoolean("before_write");
  if (enabled) { enabled = false; if (enabled) {} }
  if (enabled) {}
 }
 void after() {
  boolean enabled = Boolean.getBoolean("after_write");
  do { enabled = false; } while (enabled);
  if (enabled) {}
 }
}
"#);
    repo.write("src/start.sh", "export\tTAB_EXPORT=true\ndeclare -x DECLARE_EXPORT=false\necho $TAB_EXPORT\necho $DECLARE_EXPORT\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Getter types guard order and exports"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-type-guard-export").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-type-guard-export"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-type-guard-export"),
        )
        .await
        .unwrap();
    let flag = |key: &str| {
        response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap()
    };
    for key in ["record_key", "enum_key", "interface_key"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "reads_config")
                .count(),
            2,
            "{key}"
        );
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "guards_code")
                .count(),
            1,
            "{key}"
        );
    }
    assert_eq!(
        flag("before_write")
            .usages
            .iter()
            .filter(|usage| usage.edge_kind == "guards_code")
            .count(),
        1
    );
    assert!(
        !flag("after_write")
            .usages
            .iter()
            .any(|usage| usage.edge_kind == "guards_code")
    );
    for key in ["TAB_EXPORT", "DECLARE_EXPORT"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "defines_config")
                .count(),
            1
        );
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "reads_config")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn config_binding_boundaries_preserve_only_proven_reads_guards_and_defaults() {
    let repo = FixtureRepo::create("config-binding-boundaries");
    repo.write(
        "src/App.java",
        r#"package demo;
class Config {
 String getValue(int other) { return System.getProperty("parameter_only"); }
 String getValue() { return "ordinary"; }
 String getDirect() { return System.getProperty("direct_good"); }
}
interface Fields {
 Fake System = new Fake(); Fake Boolean = new Fake();
 default void run() {
  if (System.getProperty("false_system") != null) {}
  if (Boolean.getBoolean("false_boolean")) {}
  if (java.lang.Boolean.getBoolean("qualified_good")) {}
 }
}
class App { void run(Config config) {
 if (config.getValue() != null) {}
 if (config.getDirect() != null) {}
 for (; java.lang.Boolean.getBoolean("for_guard"); ) {}
}}
"#,
    );
    repo.write("src/start.sh", "LOCAL=false\necho $LOCAL\nf() { local INNER=false; echo $INNER; }\nexport EXPORTED=true\necho $EXPORTED\necho ${EXTERNAL:-false}\n");
    repo.write("src/config.ctmpl", "{{ keyOrDefault \"template_default\" \"true\" }}\n{{ keyOrDefault \"template_dynamic\" (env \"OTHER\") }}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Configuration binding boundaries"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-binding-boundaries").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-binding-boundaries"),
        )
        .await
        .unwrap();
    let response = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                filtered_selector("fixture", "HEAD", "src"),
                100,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-binding-boundaries"),
        )
        .await
        .unwrap();
    let flag = |key: &str| {
        response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap()
    };
    for key in ["false_system", "false_boolean", "LOCAL", "INNER"] {
        assert!(
            !response.flags.iter().any(|flag| flag.source_key == key),
            "{key}"
        );
    }
    assert_eq!(flag("parameter_only").usages.len(), 1);
    assert!(
        flag("parameter_only").usages[0]
            .metadata
            .bindings
            .is_empty()
    );
    for key in ["direct_good", "qualified_good", "for_guard"] {
        assert_eq!(
            flag(key)
                .usages
                .iter()
                .filter(|usage| usage.edge_kind == "guards_code")
                .count(),
            1,
            "{key}"
        );
    }
    for key in ["EXPORTED", "EXTERNAL"] {
        assert!(
            flag(key)
                .usages
                .iter()
                .any(|usage| usage.edge_kind == "reads_config")
        );
    }
    assert_eq!(
        flag("template_default").usages[0]
            .metadata
            .default_value
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        flag("template_default").usages[0]
            .metadata
            .value_type
            .as_deref(),
        Some("boolean")
    );
    assert!(
        flag("template_dynamic").usages[0]
            .metadata
            .default_value
            .is_none()
    );
}
