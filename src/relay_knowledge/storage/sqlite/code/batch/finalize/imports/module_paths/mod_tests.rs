//! Direct tests for module-path indexing, normalization, and ambiguity.

use std::collections::BTreeMap;

use super::{index, normalize_join, resolve_file};
use crate::storage::sqlite::code::batch::finalize::imports::ImportResolution;

#[test]
fn module_index_exposes_language_specific_source_roots() {
    let files = BTreeMap::from([
        (
            "external_deps/cpp_sdk/include/session.hpp".to_owned(),
            "cpp".to_owned(),
        ),
        (
            "vendor/example.org/client/client.go".to_owned(),
            "go".to_owned(),
        ),
    ]);

    let paths = index(&files);

    assert!(paths.contains_key("session.hpp"));
    assert!(paths.contains_key("example.org/client/client.go"));
}

#[test]
fn module_resolution_reports_ambiguous_source_root_matches() {
    let paths = BTreeMap::from([(
        "client.py".to_owned(),
        vec![
            "src/client.py".to_owned(),
            "external_deps/client.py".to_owned(),
        ],
    )]);

    assert_eq!(
        resolve_file("client.py", true, &paths),
        ImportResolution::Ambiguous
    );
}

#[test]
fn normalized_join_rejects_absolute_and_parent_escape_paths() {
    assert_eq!(
        normalize_join("src/api", "../model/item"),
        Some("src/model/item".to_owned())
    );
    assert_eq!(normalize_join("", "../outside"), None);
    assert_eq!(normalize_join("src", "/absolute"), None);
}
