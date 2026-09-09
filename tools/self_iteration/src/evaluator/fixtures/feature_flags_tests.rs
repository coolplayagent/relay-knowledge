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
    assert_eq!(count, 11_001);
    std::fs::remove_dir_all(root).unwrap();
}
