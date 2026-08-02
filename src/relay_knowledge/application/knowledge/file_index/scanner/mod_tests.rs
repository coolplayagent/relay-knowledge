//! Unit contract for bounded local file scanning and worker isolation.

use std::{fs, path::PathBuf, time::Duration};

use crate::application::FileIndexRootConfig;

use super::super::{
    content::{FileContentEntryResult, file_content_entry},
    test_support::TempFixture,
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

    assert!(matches!(
        file_content_entry(&entry, &metadata, &canonical_root, 10, 1),
        FileContentEntryResult::ReadFailed
    ));
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

    assert!(matches!(
        file_content_entry(&entry, &metadata, &canonical_root, 10, 1),
        FileContentEntryResult::ReadFailed
    ));
}

#[tokio::test]
async fn scan_roots_marks_empty_text_content_processed_for_retirement() {
    let fixture = TempFixture::new("empty-content-retirement");
    fixture.write("docs/empty.md", " \n\t\n");

    let updates = scan_roots(
        vec![FileIndexRootConfig::new(
            "local-files",
            fixture.path().to_path_buf(),
        )],
        ScanBudget {
            max_depth: 4,
            max_file_bytes: 128,
            max_files_per_root: 10,
            excludes: Vec::new(),
        },
        9,
        Duration::from_secs(30),
    )
    .await
    .expect("scan should complete");

    let update = updates.into_iter().next().expect("one root is scanned");
    let entry = update
        .entries
        .first()
        .expect("empty file should be observed");
    assert_eq!(entry.file_name, "empty.md");
    assert!(update.content_entries.is_empty());
    assert!(!update.content_truncated);
    assert!(update.processed_content_paths.contains(&entry.path));
}

#[tokio::test]
async fn scan_roots_reports_content_read_failures_without_overflow() {
    let fixture = TempFixture::new("content-read-failure");
    let path = fixture.path().join("docs/broken.md");
    fs::create_dir_all(path.parent().expect("fixture file should have parent"))
        .expect("fixture parent should be created");
    fs::write(&path, [0xff, 0xfe, b'\n']).expect("invalid utf8 fixture should be written");

    let updates = scan_roots(
        vec![FileIndexRootConfig::new(
            "local-files",
            fixture.path().to_path_buf(),
        )],
        ScanBudget {
            max_depth: 4,
            max_file_bytes: 128,
            max_files_per_root: 10,
            excludes: Vec::new(),
        },
        9,
        Duration::from_secs(30),
    )
    .await
    .expect("scan should complete");

    let update = updates.into_iter().next().expect("one root is scanned");

    assert_eq!(update.entries.len(), 1);
    assert_eq!(update.content_read_error_count, 1);
    assert!(!update.content_truncated);
    assert_eq!(
        update.last_error.as_deref(),
        Some("file content read failed")
    );
    assert!(update.content_entries.is_empty());
    assert!(
        !update
            .processed_content_paths
            .contains(&path.to_string_lossy().to_string())
    );
}

#[test]
fn file_content_entry_keeps_excerpt_and_span_aligned() {
    let fixture = TempFixture::new("content-span");
    fixture.write("docs/note.md", "\n\n  alpha\n");
    let path = fixture.path().join("docs/note.md");
    let metadata = fs::metadata(&path).expect("metadata should load");
    let entry = file_entry("local-files", "root-a", fixture.path(), &path, &metadata);
    let canonical_root = fs::canonicalize(fixture.path()).expect("root should canonicalize");

    let content = match file_content_entry(&entry, &metadata, &canonical_root, 10, 1) {
        FileContentEntryResult::Indexed(content) => content,
        FileContentEntryResult::Skipped | FileContentEntryResult::ReadFailed => {
            panic!("content should be indexed")
        }
    };

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
