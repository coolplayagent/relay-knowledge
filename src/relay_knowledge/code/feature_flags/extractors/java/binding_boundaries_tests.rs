//! Getter type ownership and guard ordering around writes.
use super::*;

#[test]
fn static_platform_imports_keep_structured_read_namespaces_defaults_and_guards() {
    let records = facts(
        r#"import static java.lang.System.*;
import static java.lang.Boolean.getBoolean;
class App {
 void run(String getenv) {
  if (getenv("ENV_IMPORT") != null) {}
  if (getProperty("property_import", "true") != null) {}
  if (getBoolean("boolean_import")) {}
 }
}
"#,
    );
    for (key, kind) in [
        ("ENV_IMPORT", "env_var"),
        ("property_import", "config_key"),
        ("boolean_import", "config_key"),
    ] {
        let read = records
            .iter()
            .find(|record| record.source_key == key && record.edge_kind == "reads_config")
            .unwrap();
        assert_eq!(read.source_kind, kind);
        let guard = records
            .iter()
            .find(|record| record.source_key == key && record.edge_kind == "guards_code")
            .unwrap();
        assert_eq!(
            guard.metadata.read_usage_id.as_deref(),
            Some(read.usage_id.as_str())
        );
    }
    assert_eq!(
        records
            .iter()
            .find(|record| record.source_key == "property_import"
                && record.edge_kind == "reads_config")
            .unwrap()
            .metadata
            .default_value
            .as_deref(),
        Some("true")
    );
}

#[test]
fn zero_argument_getters_bind_the_nearest_record_enum_and_interface_type() {
    let records = facts(
        r#"package demo;
record RecordConfig() { String getValue() { return System.getProperty("record_key"); } }
enum EnumConfig { INSTANCE; String getValue() { return System.getProperty("enum_key"); } }
interface DefaultConfig { default String getValue() { return System.getProperty("interface_key"); } }
class Outer { record Nested() { String getValue() { return System.getProperty("nested_key"); } } }
"#,
    );
    for (key, owner) in [
        ("record_key", "RecordConfig"),
        ("enum_key", "EnumConfig"),
        ("interface_key", "DefaultConfig"),
        ("nested_key", "Outer.Nested"),
    ] {
        let read = records
            .iter()
            .find(|record| record.source_key == key)
            .unwrap();
        assert!(
            read.metadata
                .bindings
                .contains(&format!("demo.{owner}.getValue")),
            "{read:?}"
        );
    }
}

#[test]
fn guards_before_body_or_update_writes_survive_but_later_conditions_do_not() {
    let records = facts(
        r#"class App {
 void branch() {
  boolean enabled = Boolean.getBoolean("branch_key");
  if (enabled) { enabled = false; if (enabled) {} }
  if (enabled) {}
 }
 void loop() {
  boolean enabled = Boolean.getBoolean("loop_key");
  for (; enabled; enabled = false) {}
  if (enabled) {}
 }
 void after() {
  boolean enabled = Boolean.getBoolean("after_key");
  do { enabled = false; } while (enabled);
  if (enabled) {}
 }
 void condition() {
  boolean enabled = Boolean.getBoolean("condition_key");
  if ((enabled = false) || enabled) {}
 }
}"#,
    );
    for key in ["branch_key", "loop_key"] {
        assert_eq!(
            records
                .iter()
                .filter(|record| record.source_key == key && record.edge_kind == "guards_code")
                .count(),
            1,
            "{key}"
        );
    }
    for key in ["after_key", "condition_key"] {
        assert!(
            !records
                .iter()
                .any(|record| record.source_key == key && record.edge_kind == "guards_code"),
            "{key}"
        );
    }
}
