//! Direct unit contract for import path and target context classification.

use super::*;

#[test]
fn import_path_queries_require_separators_or_known_file_extensions() {
    assert!(query_looks_like_import_path("linux/debugfs.h"));
    assert!(query_looks_like_import_path("./protocol"));
    assert!(query_looks_like_import_path("shared.ts"));
    assert!(!query_looks_like_import_path(
        "org.springframework.util.ObjectUtils"
    ));
    assert!(!query_looks_like_import_path("ProviderShared"));
}

#[test]
fn mixed_import_queries_extract_one_explicit_type_identity() {
    assert_eq!(
        import_target_symbol_query("alias example.org/runtime/v1 RuntimeDescriptor"),
        Some("RuntimeDescriptor")
    );
    assert_eq!(
        import_target_symbol_query("alias example.org/runtime/v1 lower_descriptor"),
        None
    );
    assert_eq!(
        import_target_symbol_query("example.org/runtime/v1 FirstType SecondType"),
        None
    );
}

#[test]
fn import_path_identity_matches_only_its_resolved_target_suffix() {
    assert!(import_path_token_matches_target_hint(
        "example.org/runtime/v1",
        "staging/src/example.org/runtime/v1"
    ));
    assert!(!import_path_token_matches_target_hint(
        "example.org/runtime/v1",
        "src/example.org/unrelated/v1"
    ));
}

#[test]
fn target_context_extracts_header_stems_and_source_directories() {
    assert_eq!(
        target_stem("ignored", Some("include/CacheStore.hpp")).as_deref(),
        Some("cachestore")
    );
    assert_eq!(
        target_stem_terms("ignored", Some("include/CacheStore.hpp")),
        ["cachestore"]
    );
    assert_eq!(parent_dir("include/cache/store.hpp"), Some("include/cache"));
    assert!(path_has_header_extension("include/cache/store.hpp"));
    assert!(source_file_can_implement_header("store.cpp"));
}
