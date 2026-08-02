//! Direct contracts for indexed-scope filter coverage.

use super::*;

#[test]
fn path_filters_accept_canonical_directory_spellings() {
    assert!(path_matches_filter("src/lib.rs", "src/"));
    assert!(path_matches_filter("src/lib.rs", "src"));
    assert!(path_matches_filter("src/lib.rs", "."));
    assert!(path_matches_filter("src/lib.rs", "./"));
    assert!(path_matches_filter("src/lib.rs", "./src"));
    assert!(!path_matches_filter("src-other/lib.rs", "src/"));
}

#[test]
fn indexed_scope_accepts_only_equal_or_narrower_selector_filters() {
    assert!(selector_filters_fit_indexed_scope(
        &["src".to_owned()],
        &["rust".to_owned(), "cpp".to_owned()],
        &["src/domain".to_owned()],
        &["rust".to_owned()],
    ));
    assert!(!selector_filters_fit_indexed_scope(
        &["src/domain".to_owned()],
        &["rust".to_owned()],
        &["src".to_owned()],
        &["rust".to_owned()],
    ));
    assert!(!selector_filters_fit_indexed_scope(
        &["src".to_owned()],
        &["rust".to_owned()],
        &["src/domain".to_owned()],
        &["cpp".to_owned()],
    ));
}

#[test]
fn unfiltered_index_scope_covers_filtered_selectors() {
    assert!(selector_filters_fit_indexed_scope(
        &[],
        &[],
        &["src".to_owned()],
        &["rust".to_owned()],
    ));
}

#[test]
fn cpp_language_filter_includes_c_headers_only() {
    let cpp_filter = ["cpp".to_owned()];

    assert!(language_filter_allows_path(
        "include/relay.h",
        "c",
        &cpp_filter,
    ));
    assert!(!language_filter_allows_path(
        "src/relay.c",
        "c",
        &cpp_filter,
    ));
    assert!(language_filter_allows("cpp", &cpp_filter));
}

#[test]
fn empty_path_filter_allows_every_path() {
    assert!(path_filter_allows("src/lib.rs", &[]));
    assert!(path_filter_allows("src/lib.rs", &["src".to_owned()],));
    assert!(!path_filter_allows("tests/smoke.rs", &["src".to_owned()],));
}
