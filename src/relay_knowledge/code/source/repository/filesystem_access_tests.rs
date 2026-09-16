use super::*;

#[test]
fn safe_relative_paths_reject_escapes_and_empty_segments() {
    assert!(safe_relative_path("src/lib.rs"));
    assert!(!safe_relative_path("../src/lib.rs"));
    assert!(!safe_relative_path("src//lib.rs"));
    assert!(!safe_relative_path("/src/lib.rs"));
    assert!(!safe_relative_path("src\\lib.rs"));
}

#[test]
fn source_io_unknown_entry_type_revokes_the_known_parent_subtree() {
    use crate::code::{source::local_io::test_fault, test_fixtures::TempSourceDir};
    use crate::domain::{CodePathIoOperation, CodePathKind};

    let source = TempSourceDir::create("entry-type-parent");
    source.write("lib/retained.rs", "pub fn retained() {}");
    source.write("src/a.rs", "pub fn previous() {}");
    source.write("src/a_nested/b.rs", "pub fn nested() {}");
    source.write("src/z_blocked/c.rs", "pub fn blocked() {}");
    let _guard = test_fault::inject_entry_type(
        source.path.join("src/z_blocked"),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    );
    let policy = FileSystemScanPolicy::from_path_filters(&[".".to_owned()]);
    let mut skipped = Vec::new();
    let files = filesystem_files(&source.path, &policy, &mut skipped).unwrap();

    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["lib/retained.rs"]
    );
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, "src");
    assert_eq!(skipped[0].io.path_kind, CodePathKind::Directory);
    assert_eq!(skipped[0].io.operation, CodePathIoOperation::Metadata);
    let diagnostic = skipped[0].diagnostic("repo", "scope");
    for path in ["src/a.rs", "src/a_nested/b.rs", "src/z_blocked/c.rs"] {
        assert!(
            diagnostic.skips_path(path),
            "{path} must not count as confirmed deletion"
        );
    }
}

#[test]
fn source_io_unselected_entry_type_failure_does_not_invent_a_directory() {
    use crate::code::{source::local_io::test_fault, test_fixtures::TempSourceDir};
    let source = TempSourceDir::create("entry-type-language");
    source.write("src/A.java", "class A {}");
    source.write("src/ignored.txt", "not selected");
    let _guard = test_fault::inject_entry_type(
        source.path.join("src/ignored.txt"),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    );
    let policy = FileSystemScanPolicy::from_path_and_language_filters(
        &["src".to_owned()],
        &["java".to_owned()],
        &[],
    );
    let mut skipped = Vec::new();
    assert!(
        filesystem_files(&source.path, &policy, &mut skipped)
            .unwrap()
            .is_empty()
    );
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, "src");
    assert_eq!(
        skipped[0].io.path_kind,
        crate::domain::CodePathKind::Directory
    );
}

#[test]
fn source_io_unknown_root_entry_and_global_metadata_failures_remain_fatal() {
    use crate::code::{source::local_io::test_fault, test_fixtures::TempSourceDir};
    let source = TempSourceDir::create("entry-type-global");
    source.write("src/a.rs", "pub fn a() {}");
    let policy = FileSystemScanPolicy::from_path_filters(&[".".to_owned()]);
    for (path, kind) in [
        ("src", std::io::ErrorKind::PermissionDenied),
        ("src/a.rs", std::io::ErrorKind::OutOfMemory),
    ] {
        let _guard =
            test_fault::inject_entry_type(source.path.join(path), std::io::Error::from(kind));
        assert!(matches!(
            filesystem_files(&source.path, &policy, &mut Vec::new()),
            Err(CodeIndexError::Io(_))
        ));
    }
}
