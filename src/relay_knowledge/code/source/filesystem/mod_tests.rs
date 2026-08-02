use super::*;

#[test]
fn default_filesystem_scope_allows_supported_roots_and_rejects_broad_segments() {
    assert!(filesystem_default_source_allows("src/lib.rs"));
    assert!(filesystem_default_source_allows("README.md"));
    assert!(!filesystem_default_source_allows("vendor/lib.rs"));
    assert!(!filesystem_default_source_allows("src/logo.png"));
}

#[test]
fn path_normalization_and_default_exclusions_are_explicit() {
    assert_eq!(normalize_path_filter(" ./src/lib/ "), "src/lib");
    assert!(source_default_file_preset_excludes("assets/logo.png"));
    assert!(source_default_file_preset_excludes("uv.lock"));
    assert!(!source_default_file_preset_excludes("src/lib.rs"));
}
