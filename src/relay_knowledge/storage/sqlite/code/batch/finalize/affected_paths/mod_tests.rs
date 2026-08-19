use super::*;

#[test]
fn full_scope_when_changed_paths_empty() {
    let result = AffectedPaths::full_scope();
    assert!(result.is_full_scope());
    assert!(result.path_refs().is_empty());
}

#[test]
fn path_refs_returns_str_slices() {
    let result = AffectedPaths {
        paths: vec!["a/b.py".to_owned(), "c/d.py".to_owned()],
        fallback_to_full_scope: false,
    };
    assert_eq!(result.path_refs(), vec!["a/b.py", "c/d.py"]);
}
