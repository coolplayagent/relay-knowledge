#[test]
fn generated_configuration_scope_exceeds_analysis_budget_without_large_files() {
    let root = std::env::temp_dir().join(format!(
        "config-width-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    super::write(&root).unwrap();
    let mut count = 0;
    for entry in std::fs::read_dir(root.join("config")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(text.len() < 32 * 1024);
        count += text.lines().count();
    }
    assert_eq!(count, 15_403);
    let query_noise = std::fs::read_to_string(root.join("config/query_noise_00.yaml")).unwrap();
    let metadata_noise =
        std::fs::read_to_string(root.join("config/metadata_noise_00.properties")).unwrap();
    assert_eq!(query_noise.matches("metadata_needle").count(), 100);
    assert_eq!(metadata_noise.matches("domain=selected").count(), 100);
    assert!(!metadata_noise.contains("metadata_needle"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_binding_group_fixture_keeps_query_and_metadata_on_distinct_usages() {
    let root = std::env::temp_dir().join(format!(
        "config-binding-width-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    super::write_binding_groups(&root).unwrap();
    let noise = std::fs::read_to_string(root.join("noise.yaml")).unwrap();
    assert_eq!(noise.matches("aaa_needle_").count(), 1100);
    assert_eq!(noise.matches("aab_unrelated_").count(), 1100);
    let properties = std::fs::read_to_string(root.join("settings.properties")).unwrap();
    assert!(properties.contains("domain=alias-selected"));
    assert!(!properties.contains("NeedleReader"));
    let java = std::fs::read_to_string(root.join("NeedleReader.java")).unwrap();
    assert!(java.contains("System.getProperty(Keys.FIRST)"));
    assert!(!java.contains("domain=alias-selected"));
    std::fs::remove_dir_all(root).unwrap();
}
