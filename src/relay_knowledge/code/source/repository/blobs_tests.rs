use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn filesystem_snapshot_batch_reads_paths_in_request_order() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-repository-blobs-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(root.join("src")).expect("temporary source root should be created");
    std::fs::write(root.join("src/a.rs"), b"a").expect("first source should be written");
    std::fs::write(root.join("src/b.rs"), b"b").expect("second source should be written");

    let blobs = source_snapshot_batch_bytes(
        &root,
        RepositorySourceKind::FileSystem,
        "filesystem:test",
        &["src/b.rs".to_owned(), "src/a.rs".to_owned()],
    )
    .expect("filesystem batch should load");
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(blobs, [b"b".to_vec(), b"a".to_vec()]);
}
