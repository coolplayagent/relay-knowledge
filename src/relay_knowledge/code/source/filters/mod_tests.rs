use super::*;

#[test]
fn filesystem_filter_sets_keep_broad_and_source_roots_separate() {
    for broad in [".git", "build", "node_modules", "target", "vendor"] {
        assert!(FILESYSTEM_BROAD_SEGMENTS.contains(&broad));
        assert!(!FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(&broad));
    }
    for root in FILESYSTEM_AUTO_DISCOVERY_FILTERS {
        assert!(FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(root));
    }
}

#[test]
fn default_file_exclusions_cover_binary_artifacts_and_lock_noise() {
    for extension in ["png", "pdf", "wasm", "zip"] {
        assert!(DEFAULT_EXCLUDED_EXTENSIONS.contains(&extension));
    }
    assert_eq!(DEFAULT_EXCLUDED_FILENAMES, ["uv.lock"]);
}
