use super::*;

#[test]
fn incremental_paths_must_stay_inside_the_generated_repository() {
    assert_eq!(
        safe_incremental_path("src/new.rs").expect("safe path"),
        Path::new("src/new.rs")
    );
    for path in ["", ".", "../outside", "/absolute", "nested\\outside"] {
        assert!(safe_incremental_path(path).is_err(), "{path:?} should fail");
    }
}
