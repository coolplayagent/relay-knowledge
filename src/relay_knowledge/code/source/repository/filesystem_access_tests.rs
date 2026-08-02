use super::*;

#[test]
fn safe_relative_paths_reject_escapes_and_empty_segments() {
    assert!(safe_relative_path("src/lib.rs"));
    assert!(!safe_relative_path("../src/lib.rs"));
    assert!(!safe_relative_path("src//lib.rs"));
    assert!(!safe_relative_path("/src/lib.rs"));
    assert!(!safe_relative_path("src\\lib.rs"));
}
