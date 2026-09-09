#[test]
fn fixture_contains_many_unrelated_calls_and_a_unique_target() {
    let root = std::env::temp_dir().join(format!(
        "canonical-call-fixture-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    super::write(&root).unwrap();
    assert_eq!(std::fs::read_dir(root.join("src")).unwrap().count(), 34);
    assert_eq!(
        std::fs::read_to_string(root.join("src/Noise0.java"))
            .unwrap()
            .matches("System.nanoTime()")
            .count(),
        64
    );
    std::fs::remove_dir_all(root).unwrap();
}
