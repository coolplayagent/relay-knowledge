use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn class_call_fixture_keeps_member_calls_and_wide_noise_separate() {
    let root = std::env::temp_dir().join(format!(
        "class-call-fixture-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    write_java_class_calls(&root).unwrap();
    assert_eq!(fs::read_dir(root.join("src")).unwrap().count(), 258);
    assert!(
        fs::read_to_string(root.join("src/Caller.java"))
            .unwrap()
            .contains("Processor.processItem();")
    );
    let noise = fs::read_to_string(root.join("src/Noise255.java")).unwrap();
    assert_eq!(noise.matches("println").count(), 128);
    assert!(!noise.contains("processItem"));
    fs::remove_dir_all(root).unwrap();
}
