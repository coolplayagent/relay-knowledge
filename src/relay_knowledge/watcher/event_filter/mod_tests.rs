//! Direct tests for watcher event admission.

use super::*;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from("/project")
}

#[test]
fn allows_rust_source_file() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(filter.should_process_path(&root().join("src/main.rs")));
}

#[test]
fn rejects_git_directory() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&root().join(".git/HEAD")));
}

#[test]
fn rejects_node_modules() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&root().join("node_modules/foo/index.js")));
}

#[test]
fn rejects_binary_file() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&root().join("image.png")));
}

#[test]
fn allows_dockerfile_without_extension() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(filter.should_process_path(&root().join("Dockerfile")));
}

#[test]
fn path_filter_accepts_matching_prefix() {
    let filter = WatcherEventFilter::new(root(), vec!["src/".to_owned()], vec![]);
    assert!(filter.should_process_path(&root().join("src/lib.rs")));
    assert!(!filter.should_process_path(&root().join("docs/README.md")));
}

#[test]
fn rejects_path_outside_root() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&PathBuf::from("/other/project/main.rs")));
}

#[test]
fn rejects_target_directory() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&root().join("target/debug/lib.rs")));
}

#[test]
fn default_filter_rejects_empty_path() {
    let filter = WatcherEventFilter::default();
    assert!(!filter.should_process_path(&PathBuf::from("main.rs")));
}

#[test]
fn allows_python_file() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(filter.should_process_path(&root().join("app/models.py")));
}

#[test]
fn rejects_pycache() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(!filter.should_process_path(&root().join("__pycache__/foo.cpython-311.pyc")));
}

#[test]
fn allows_known_language_alias_extensions() {
    let filter = WatcherEventFilter::new(root(), vec![], vec![]);
    assert!(filter.should_process_path(&root().join("src/lib.cc")));
    assert!(filter.should_process_path(&root().join("web/app.mjs")));
    assert!(filter.should_process_path(&root().join("build.gradle.kts")));
}

#[test]
fn language_filters_accept_config_and_document_languages() {
    for (language, path) in [
        ("json", "config/app.json"),
        ("yaml", "config/app.yaml"),
        ("toml", "Cargo.toml"),
        ("sql", "schema/main.sql"),
        ("markdown", "README.md"),
    ] {
        let filter = WatcherEventFilter::new(root(), vec![], vec![language.to_owned()]);
        assert!(filter.should_process_path(&root().join(path)));
    }
}

#[test]
fn unknown_language_filter_does_not_match_extension_by_name() {
    let filter = WatcherEventFilter::new(root(), vec![], vec!["rs".to_owned()]);
    assert!(!filter.should_process_path(&root().join("src/main.rs")));
}
