//! Direct tests for source-root-aware symbol path matching.

use super::path_matches_candidate;

#[test]
fn symbol_paths_match_normalized_source_root_candidates() {
    assert!(path_matches_candidate(
        "external_deps/python_sdk/client.py",
        "external_deps/python_sdk/client.py"
    ));
    assert!(path_matches_candidate(
        "external_deps/python_sdk/client.py",
        "python_sdk/client.py"
    ));
    assert!(path_matches_candidate(
        "external_deps/python_sdk/client.py",
        "./python_sdk/client.py/"
    ));
}

#[test]
fn symbol_paths_reject_unrelated_module_candidates() {
    assert!(!path_matches_candidate(
        "external_deps/python_sdk/client.py",
        "other_sdk/client.py"
    ));
}
