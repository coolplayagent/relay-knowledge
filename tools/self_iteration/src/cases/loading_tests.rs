use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

struct CaseTree {
    root: PathBuf,
}

impl CaseTree {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-case-loading-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("case tree should be created");
        Self { root }
    }

    fn write(&self, path: &str, content: &str) -> PathBuf {
        let path = self.root.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("case parent should be created");
        }
        std::fs::write(&path, content).expect("case file should be written");
        path
    }
}

impl Drop for CaseTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn recursively_loads_and_merges_relative_case_files() {
    let tree = CaseTree::new();
    tree.write(
        "included/nested.json",
        r#"{"query_cases":[{"id":"nested"}],"repositories":{"nested":{}}}"#,
    );
    tree.write(
        "included/base.json",
        r#"{
            "include_files":["nested.json"],
            "query_cases":[{"id":"base"}],
            "repositories":{"base":{}}
        }"#,
    );
    let root = tree.write(
        "cases.json",
        r#"{
            "include_files":["included/base.json"],
            "query_cases":[{"id":"root"}],
            "repositories":{"root":{}}
        }"#,
    );

    let cases = load_cases(&root).expect("case files should load");

    assert_eq!(cases["query_cases"].as_array().map(Vec::len), Some(3));
    let repositories = cases["repositories"]
        .as_object()
        .expect("repositories should remain an object");
    assert_eq!(repositories.len(), 3);
    assert!(cases.get("include_files").is_none());
}

#[test]
fn rejects_non_string_include_entries() {
    let tree = CaseTree::new();
    let root = tree.write("cases.json", r#"{"include_files":[42]}"#);

    let error = load_cases(&root).expect_err("numeric include should fail");

    assert!(error.contains("invalid include file entry"));
    assert!(error.contains(root.to_string_lossy().as_ref()));
}

#[test]
fn reports_missing_and_invalid_case_files_with_their_paths() {
    let tree = CaseTree::new();
    let missing = tree.root.join("missing.json");
    let missing_error = load_cases(&missing).expect_err("missing file should fail");
    assert!(missing_error.contains("failed to read"));
    assert!(missing_error.contains(missing.to_string_lossy().as_ref()));

    let invalid = tree.write("invalid.json", "{");
    let invalid_error = load_cases(&invalid).expect_err("invalid JSON should fail");
    assert!(invalid_error.contains("failed to parse"));
    assert!(invalid_error.contains(invalid.to_string_lossy().as_ref()));
}

#[test]
fn rejects_included_case_files_with_non_object_roots() {
    let tree = CaseTree::new();
    tree.write("included.json", "[]");
    let root = tree.write("cases.json", r#"{"include_files":["included.json"]}"#);

    let error = load_cases(&root).expect_err("non-object include should fail");

    assert_eq!(error, "case config roots must be objects");
}

#[test]
fn rejects_isolated_repository_set_members_after_include_merge() {
    let tree = CaseTree::new();
    tree.write(
        "included/repositories.json",
        r#"{
            "repositories": {
                "member_a": {"isolated_index_home": true}
            }
        }"#,
    );
    let root = tree.write(
        "cases.json",
        r#"{
            "include_files": ["included/repositories.json"],
            "repository_sets": {
                "workspace": {
                    "members": [{"repository": "member_a"}]
                }
            }
        }"#,
    );

    let error = load_cases(&root).expect_err("isolated set member should fail after merge");

    assert!(error.contains("repository set \"workspace\""));
    assert!(error.contains("member \"member_a\""));
    assert!(error.contains("must share one evaluation home"));
}

#[test]
fn checked_in_case_matrix_isolates_heavy_repositories_but_keeps_one_shared_regression() {
    let cases_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cases.json");
    let cases = load_cases(&cases_path).expect("checked-in case matrix should be valid");
    let repositories = cases["repositories"]
        .as_object()
        .expect("repositories should be configured");
    let isolated_repositories = [
        "relay_teams",
        "opencode_typescript",
        "linux_sample",
        "linux_full",
        "kubernetes_go_sample",
        "spring_framework_java",
        "rustfs_rust",
        "codex_python",
        "nvm_bash",
        "dotnet_runtime_csharp",
        "okhttp_kotlin",
        "laravel_php",
        "rails_ruby",
        "scala3_scala",
        "alamofire_swift",
    ];
    for name in isolated_repositories {
        assert_eq!(
            repositories[name]["isolated_index_home"].as_bool(),
            Some(true),
            "{name} must keep cold-index measurements isolated"
        );
    }
    assert_eq!(
        repositories["leveldb_cpp"]["isolated_index_home"].as_bool(),
        None,
        "the small LevelDB workload intentionally protects shared-order behavior"
    );
    for name in [
        "temporal_samples_go",
        "temporal_sdk_go",
        "otel_collector_contrib",
        "otel_collector",
    ] {
        assert_ne!(
            repositories[name]["isolated_index_home"].as_bool(),
            Some(true),
            "repository-set member {name} must use the shared evaluation home"
        );
    }
}
