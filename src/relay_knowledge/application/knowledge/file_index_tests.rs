use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    api::FileIndexRequest,
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::{KnowledgeStore, SqliteGraphStore, StorageError},
};

use super::*;

#[tokio::test]
async fn scan_roots_respects_budget_excludes_and_metadata() {
    let fixture = TempFixture::new("scan-budget");
    fixture.write("docs/report.pdf", "pdf");
    fixture.write("target/generated.txt", "generated");
    fixture.write(".hidden/secret.txt", "secret");
    fixture.write("deep/a/b/c/too-deep.txt", "deep");
    fixture.write("large.bin", "too large for budget");
    fixture.write("notes/skipme.txt", "configured exclusion");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/", fixture.path().join("escape"))
        .expect("symlink fixture should be created");

    let updates = scan_roots(
        vec![FileIndexRootConfig::new(
            "local-files",
            fixture.path().to_path_buf(),
        )],
        ScanBudget {
            max_depth: 2,
            max_file_bytes: 8,
            max_files_per_root: 10,
            excludes: vec!["skipme".to_owned()],
        },
        42,
        Duration::from_secs(30),
    )
    .await
    .expect("scan should complete");

    let update = updates.into_iter().next().expect("one root is scanned");
    assert_eq!(update.root.scope_id, "local-files");
    assert_eq!(update.now_ms, 42);
    assert!(update.truncated);
    assert_eq!(update.scan_error_count, 0);
    assert_eq!(update.entries.len(), 1);
    let entry = &update.entries[0];
    assert_eq!(entry.file_name, "report.pdf");
    assert_eq!(entry.extension.as_deref(), Some("pdf"));
    assert!(entry.relative_path.ends_with("docs/report.pdf"));
    assert!(entry.parent_dir.ends_with("docs"));
    assert_eq!(entry.size_bytes, 3);
    assert!(entry.fingerprint.starts_with("3:"));
}

#[cfg(unix)]
#[test]
fn file_content_entry_does_not_follow_symlink_at_read_time() {
    let fixture = TempFixture::new("content-symlink");
    let target = fixture.path().join("outside.md");
    fixture.write("outside.md", "outside secret");
    let link = fixture.path().join("docs/link.md");
    fs::create_dir_all(link.parent().expect("link should have parent"))
        .expect("link parent should be created");
    std::os::unix::fs::symlink(&target, &link).expect("symlink should be created");
    let metadata = fs::metadata(&target).expect("target metadata should load");
    let entry = file_entry("local-files", "root-a", fixture.path(), &link, &metadata);
    let canonical_root = fs::canonicalize(fixture.path()).expect("root should canonicalize");

    assert!(file_content_entry(&entry, &metadata, &canonical_root, 10, 1).is_none());
}

#[cfg(unix)]
#[test]
fn file_content_entry_rejects_ancestor_symlink_escape() {
    let root = TempFixture::new("content-ancestor-root");
    let outside = TempFixture::new("content-ancestor-outside");
    outside.write("secret.md", "outside secret");
    let link_dir = root.path().join("docs");
    std::os::unix::fs::symlink(outside.path(), &link_dir)
        .expect("ancestor symlink should be created");
    let path = link_dir.join("secret.md");
    let metadata = fs::metadata(&path).expect("symlink target metadata should load");
    let entry = file_entry("local-files", "root-a", root.path(), &path, &metadata);
    let canonical_root = fs::canonicalize(root.path()).expect("root should canonicalize");

    assert!(file_content_entry(&entry, &metadata, &canonical_root, 10, 1).is_none());
}

#[test]
fn content_scan_budget_accounts_attempted_reads() {
    let mut content_scan_bytes = 0;

    assert!(!file_content_budget::reserve_content_read_with_budget(
        &mut content_scan_bytes,
        4,
        6
    ));
    assert!(file_content_budget::reserve_content_read_with_budget(
        &mut content_scan_bytes,
        4,
        6
    ));
    assert_eq!(content_scan_bytes, 6);
}

#[test]
fn file_content_entry_keeps_excerpt_and_span_aligned() {
    let fixture = TempFixture::new("content-span");
    fixture.write("docs/note.md", "\n\n  alpha\n");
    let path = fixture.path().join("docs/note.md");
    let metadata = fs::metadata(&path).expect("metadata should load");
    let entry = file_entry("local-files", "root-a", fixture.path(), &path, &metadata);
    let canonical_root = fs::canonicalize(fixture.path()).expect("root should canonicalize");

    let content = file_content_entry(&entry, &metadata, &canonical_root, 10, 1)
        .expect("content should be indexed");

    assert_eq!(content.chunks.len(), 1);
    assert_eq!(content.chunks[0].start_byte, 0);
    assert_eq!(content.chunks[0].start_line, 1);
    assert_eq!(content.chunks[0].content, "\n\n  alpha\n");
}

#[tokio::test]
async fn scan_roots_reports_missing_roots_and_file_count_truncation() {
    let fixture = TempFixture::new("scan-truncated");
    fixture.write("first.txt", "one");
    fixture.write("second.txt", "two");
    let missing = fixture.path().join("missing");

    let updates = scan_roots(
        vec![
            FileIndexRootConfig::new("local-files", fixture.path().to_path_buf()),
            FileIndexRootConfig::new("local-files", missing),
        ],
        ScanBudget {
            max_depth: 4,
            max_file_bytes: 128,
            max_files_per_root: 1,
            excludes: Vec::new(),
        },
        7,
        Duration::from_secs(30),
    )
    .await
    .expect("scan should complete");

    let truncated = updates
        .iter()
        .find(|update| update.root.root_path == fixture.path().to_string_lossy())
        .expect("fixture root should be present");
    assert!(truncated.truncated);
    assert_eq!(truncated.entries.len(), 1);

    let missing = updates
        .iter()
        .find(|update| update.root.root_path.ends_with("missing"))
        .expect("missing root should be reported");
    assert_eq!(missing.scan_error_count, 1);
    assert!(missing.entries.is_empty());
    assert!(missing.last_error.is_some());
}

#[tokio::test]
async fn scan_timeout_returns_degraded_root_update() {
    let fixture = TempFixture::new("scan-timeout");

    let update = scan_root_with_timeout(
        FileIndexRootConfig::new("local-files", fixture.path().to_path_buf()),
        ScanBudget {
            max_depth: 4,
            max_file_bytes: 128,
            max_files_per_root: 1,
            excludes: Vec::new(),
        },
        9,
        Duration::ZERO,
    )
    .await
    .expect("timeout update should be produced");

    assert_eq!(update.scan_error_count, 1);
    assert!(update.truncated);
    assert!(update.entries.is_empty());
    assert_eq!(
        update.last_error.as_deref(),
        Some("file index scan timed out")
    );
}

#[test]
fn scan_worker_busy_update_reports_bounded_backpressure() {
    let update = scan_worker_busy_file_index_root_update(
        FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs")),
        11,
    );

    assert_eq!(update.scan_error_count, 1);
    assert!(update.truncated);
    assert!(update.entries.is_empty());
    assert_eq!(
        update.last_error.as_deref(),
        Some("file index scan worker is still busy")
    );
    assert_eq!(update.now_ms, 11);
}

#[test]
fn query_validation_helpers_reject_unbounded_inputs() {
    assert_eq!(required_query("  quarter  ".to_owned()).unwrap(), "quarter");
    assert!(required_query(" \t ".to_owned()).is_err());
    assert_eq!(bounded_limit(1).unwrap(), 1);
    assert!(bounded_limit(0).is_err());
    assert!(bounded_limit(MAX_FILE_QUERY_LIMIT + 1).is_err());
    assert_eq!(
        normalize_optional_text(Some(" root ".to_owned())).unwrap(),
        Some("root".to_owned())
    );
    assert!(normalize_optional_text(Some(" ".to_owned())).is_err());
    assert_eq!(normalize_optional_text(None).unwrap(), None);
}

#[test]
fn query_timeout_helpers_map_runtime_budget_and_storage_errors() {
    assert_eq!(query_timeout_ms(std::time::Duration::from_millis(125)), 125);
    assert!(storage_error_timed_out(&StorageError::InvalidInput(
        "file query timed out waiting for storage lock".to_owned()
    )));
    assert!(!storage_error_timed_out(&StorageError::InvalidInput(
        "different validation failure".to_owned()
    )));
}

#[tokio::test]
async fn explicit_roots_must_match_authorized_runtime_roots() {
    let fixture = TempFixture::new("authorized-roots");
    let service = service_for_root(fixture.path()).await;
    let authorized = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec![fixture.path().join(".").to_string_lossy().to_string()],
        })
        .expect("configured root spelling should be authorized");
    assert_eq!(authorized.len(), 1);

    let denied = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec![fixture.path().join("other").to_string_lossy().to_string()],
        })
        .expect_err("unconfigured root should be denied");
    assert!(denied.contains("is not configured"));

    let relative = service
        .file_index_roots_from_request(FileIndexRequest {
            source_scope: Some("local-files".to_owned()),
            roots: vec!["relative/docs".to_owned()],
        })
        .expect_err("relative roots should be denied");
    assert!(relative.contains("absolute path"));
}

#[tokio::test]
async fn same_path_roots_remain_distinct_across_scopes() {
    let fixture = TempFixture::new("scope-roots");
    let home = fixture.path().join("home");
    let documents = home.join("Documents");
    fs::create_dir_all(&documents).expect("documents directory should be created");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", home.to_string_lossy().to_string()),
            ("TMPDIR", "/tmp".to_owned()),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                documents.to_string_lossy().to_string(),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS",
                "120000".to_owned(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    assert_eq!(runtime.file_index.scan_timeout, Duration::from_secs(120));

    let matching_roots = runtime
        .file_index
        .roots
        .iter()
        .filter(|root| root.root_path.as_path() == documents.as_path())
        .collect::<Vec<_>>();
    assert_eq!(matching_roots.len(), 2);
    assert_ne!(matching_roots[0].scope_id, matching_roots[1].scope_id);
}

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new(name: &str) -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");

        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn service_for_root(root: &Path) -> RelayKnowledgeService {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("home should be created");
    let relay_home = root.join("relay");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", home.to_string_lossy().to_string()),
            ("TMPDIR", "/tmp".to_owned()),
            (
                "RELAY_KNOWLEDGE_HOME",
                relay_home.to_string_lossy().to_string(),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                root.to_string_lossy().to_string(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"))
        as Arc<dyn KnowledgeStore>;

    RelayKnowledgeService::with_store(runtime, store)
}
