use super::facts;

#[test]
fn escaped_keys_decode_once_and_constants_keep_runtime_identity() {
    let records = facts(
        r#"class Keys {
 static final String KEY="\146lag";
 void run() {
  System.getProperty("\u0066lag", "true");
  System.getProperty(KEY, "true");
  System.getProperty("\\u0066lag", "distinct");
 }
}"#,
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "flag" && r.edge_kind == "reads_config")
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "flag" && r.edge_kind == "binds_config_symbol")
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == r"\u0066lag" && r.edge_kind == "reads_config")
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "Keys.KEY" && r.source_kind == "config_symbol")
    );
}

#[test]
fn qualified_platform_receivers_require_unshadowed_root_but_static_imports_do_not() {
    let records = facts(
        r#"import static java.lang.System.getenv;
 class App {
 void run(Object java) {
  java.lang.System.getProperty("false_property");
  java.lang.System.getenv("false_environment");
  java.lang.Boolean.getBoolean("false_boolean");
  getenv("imported_real");
 }
 void real() { java.lang.System.getProperty("qualified_real"); }
 }"#,
    );
    assert!(records.iter().all(|r| !r.source_key.starts_with("false_")));
    assert!(records.iter().any(|r| r.source_key == "imported_real"));
    assert!(records.iter().any(|r| r.source_key == "qualified_real"));
}

#[test]
fn inherited_constants_keep_local_parameter_and_own_field_shadows() {
    let records = facts(
        r#"interface Keys { String FLAG="inherited"; }
 class App implements Keys {
 void real() { System.getProperty(FLAG); }
 void parameter(String FLAG) { System.getProperty(FLAG); }
 void local() { String FLAG="ordinary"; System.getProperty(FLAG); }
 }
 class Own implements Keys { static final String FLAG="own"; void run(){System.getProperty(FLAG);} }
 "#,
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.source_key == "Keys.FLAG" && r.edge_kind == "reads_config")
            .count(),
        1
    );
    assert!(
        records
            .iter()
            .any(|r| r.source_key == "Own.FLAG" && r.edge_kind == "reads_config")
    );
}
