//! Enforce the physical configuration owners declared by hard constraint 3.8.
use super::*;

#[test]
fn configuration_helpers_stay_under_named_physical_owners() {
    let root = source_root().join("code/config_files");
    let mut actual = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    actual.sort();
    let expected = [
        "calls",
        "detection",
        "key_values",
        "knowledge_map",
        "languages",
        "mod.rs",
        "model",
        "source",
    ];
    assert_eq!(
        actual, expected,
        "config_files root must contain only its facade and specified physical owners"
    );
    for owner in expected.into_iter().filter(|name| *name != "mod.rs") {
        assert!(root.join(owner).is_dir());
        assert!(root.join(owner).join("mod.rs").is_file());
    }
}
