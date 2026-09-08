//! Bounded Windows service definition parsing and path precedence.

use super::*;

fn fixture() -> (PathBuf, RuntimePaths, String) {
    let root = std::env::temp_dir().join(format!(
        "relay-restored-paths-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(RELAY_KNOWLEDGE_HOME, root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let definition = format!(
        "<service><env name=\"RELAY_KNOWLEDGE_DATA_DIR\" value=\"{}\"/></service>",
        quick_xml::escape::escape(paths.data_dir.to_str().unwrap())
    );
    (root, paths, definition)
}

#[test]
fn restored_definition_decodes_pinned_storage_and_retains_sid_policy() {
    let (root, paths, definition) = fixture();
    assert_eq!(
        paths
            .with_service_storage_overrides(&storage_overrides(&definition).unwrap())
            .unwrap()
            .data_dir,
        paths.data_dir
    );
    let overrides = storage_overrides(r#"<service><env name="RELAY_KNOWLEDGE_HOME" value="C:/old &amp; private"/><env name="RELAY_KNOWLEDGE_DATA_DIR" value="D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data"></env></service>"#).unwrap();
    if cfg!(windows) {
        let restored = paths.with_service_storage_overrides(&overrides).unwrap();
        assert_eq!(
            restored.windows_data_sid.as_deref(),
            Some("S-1-5-21-1-2-3-1001")
        );
        assert_eq!(restored.config_dir, paths.config_dir);
    } else {
        assert!(paths.with_service_storage_overrides(&overrides).is_err());
    }
    let home = storage_overrides(
        r#"<service><env name="relay_knowledge_home" value="C:/old &amp; private"/></service>"#,
    )
    .unwrap();
    assert_eq!(home.home, Some(PathBuf::from("C:/old & private")));
    let portable_home = PathEnvOverrides {
        home: Some(root.clone()),
        ..Default::default()
    };
    assert_eq!(
        paths
            .with_service_storage_overrides(&portable_home)
            .unwrap()
            .data_dir,
        root.join("data")
    );
    assert!(
        paths
            .with_service_storage_overrides(&PathEnvOverrides::default())
            .is_err()
    );
}

#[test]
fn ambiguous_or_unbounded_service_definitions_are_rejected() {
    for definition in [
        "<service/>",
        "<other></other>",
        r#"<!DOCTYPE service><service><env name="RELAY_KNOWLEDGE_HOME" value="C:/old"/></service>"#,
        r#"<service><env name="RELAY_KNOWLEDGE_HOME"/></service>"#,
        r#"<service><env name="RELAY_KNOWLEDGE_HOME" value="C:/a"/><env name="relay_knowledge_home" value="C:/b"/></service>"#,
        r#"<service><env name="RELAY_KNOWLEDGE_HOME" value="C:/old"/>"#,
        r#"<service><env name="RELAY_KNOWLEDGE_HOME" value="&unknown;"/></service>"#,
        r#"<service><nested><env name="RELAY_KNOWLEDGE_HOME" value="C:/old"/></nested></service>"#,
    ] {
        assert!(storage_overrides(definition).is_err(), "{definition}");
    }
    assert!(
        storage_overrides(&format!(
            "<service>{}{}</service>",
            "<nested>".repeat(33),
            "</nested>".repeat(33)
        ))
        .is_err()
    );
}
