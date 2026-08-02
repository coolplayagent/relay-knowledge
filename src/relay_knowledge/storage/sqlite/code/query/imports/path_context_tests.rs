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
