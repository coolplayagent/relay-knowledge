use super::*;
use crate::code::source::local_io::test_fault;
#[test]
fn source_io_direct_full_snapshot_skips_an_unreadable_file_without_empty_source() {
    let root = std::env::temp_dir().join(format!(
        "relay-direct-io-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), "pub fn present() {}").unwrap();
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        root.to_string_lossy(),
        vec!["src".into()],
        vec![],
    )
    .unwrap();
    let selector = CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap();
    let complete =
        super::super::build_full_snapshot(&registration, &selector, &root, &Default::default())
            .unwrap();
    let guard = test_fault::inject(
        root.join("src/a.rs"),
        CodePathIoOperation::Read,
        0,
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "read blocked"),
    );
    let partial =
        super::super::build_full_snapshot(&registration, &selector, &root, &Default::default())
            .unwrap();
    assert_ne!(partial.source_scope, complete.source_scope);
    assert!(partial.files.is_empty());
    assert!(partial.symbols.is_empty());
    assert!(partial.chunks.is_empty());
    assert_eq!(partial.diagnostics.len(), 1);
    drop(guard);
    assert_eq!(
        super::super::build_full_snapshot(&registration, &selector, &root, &Default::default())
            .unwrap()
            .source_scope,
        complete.source_scope
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_io_pinned_direct_snapshot_rejects_a_failure_after_hashing() {
    let source = crate::code::test_fixtures::TempSourceDir::create("pinned-direct-io");
    source.write("src/a.rs", "pub fn retained() {}\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        source.path.to_string_lossy(),
        vec!["src".into()],
        vec![],
    )
    .unwrap();
    let mut selector = CodeRepositorySelector::new("fixture", "HEAD", vec![], vec![]).unwrap();
    let complete = super::super::build_full_snapshot(
        &registration,
        &selector,
        &source.path,
        &Default::default(),
    )
    .unwrap();
    selector.ref_selector = complete.resolved_commit_sha;
    let _fault = test_fault::inject(
        source.path.join("src/a.rs"),
        CodePathIoOperation::Read,
        1,
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied after hashing"),
    );
    let error = super::super::build_full_snapshot(
        &registration,
        &selector,
        &source.path,
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains(&selector.ref_selector));
    assert!(error.to_string().contains("no longer matches"));
}
