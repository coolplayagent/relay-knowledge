use super::*;

#[test]
fn source_io_local_failures_are_typed_and_root_failures_remain_fatal() {
    for kind in [
        ErrorKind::NotFound,
        ErrorKind::PermissionDenied,
        ErrorKind::Unsupported,
        ErrorKind::InvalidInput,
        ErrorKind::NotADirectory,
        ErrorKind::IsADirectory,
    ] {
        assert!(
            SkippedSourcePath::from_error(
                "src/A.java",
                CodePathKind::File,
                CodePathIoOperation::Read,
                Error::from(kind)
            )
            .is_ok()
        );
        assert!(
            SkippedSourcePath::from_error(
                "",
                CodePathKind::Directory,
                CodePathIoOperation::ReadDirectory,
                Error::from(kind)
            )
            .is_err()
        );
    }
    for kind in [
        ErrorKind::OutOfMemory,
        ErrorKind::TimedOut,
        ErrorKind::Interrupted,
        ErrorKind::Other,
        ErrorKind::BrokenPipe,
        ErrorKind::StorageFull,
    ] {
        assert!(
            SkippedSourcePath::from_error(
                "src/A.java",
                CodePathKind::File,
                CodePathIoOperation::Read,
                Error::from(kind)
            )
            .is_err()
        );
    }
}

#[test]
fn source_io_directory_boundary_and_identity_do_not_depend_on_localized_messages() {
    let make = |message| {
        SkippedSourcePath::from_error(
            "src/locked",
            CodePathKind::Directory,
            CodePathIoOperation::ReadDirectory,
            Error::new(ErrorKind::PermissionDenied, message),
        )
        .unwrap()
    };
    let first = make("access denied");
    let second = make("拒绝访问");
    assert!(first.covers("src/locked/A.java"));
    assert!(first.covers("src/locked"));
    assert!(!first.covers("src/locked-other/A.java"));
    let mut left = Vec::new();
    let mut right = Vec::new();
    first.append_identity(&mut left);
    second.append_identity(&mut right);
    assert_eq!(left, right);
    let diagnostic = first.diagnostic("repo", "scope");
    assert_eq!(diagnostic.parse_status, CodeParseStatus::Failed);
    assert_eq!(diagnostic.io.unwrap().path_kind, CodePathKind::Directory);
}

#[cfg(windows)]
#[test]
fn source_io_windows_local_codes_are_isolated_but_device_errors_are_fatal() {
    for code in [1, 2, 3, 5, 32, 33, 50, 123, 206] {
        let skipped = SkippedSourcePath::from_error(
            "src/A.java",
            CodePathKind::File,
            CodePathIoOperation::Read,
            Error::from_raw_os_error(code),
        )
        .unwrap();
        assert_eq!(skipped.io.raw_os_error, Some(code));
    }
    for code in [8, 21, 23, 31, 112, 1117, 1450, 1455] {
        assert!(
            SkippedSourcePath::from_error(
                "src/A.java",
                CodePathKind::File,
                CodePathIoOperation::Read,
                Error::from_raw_os_error(code)
            )
            .is_err(),
            "{code}"
        );
    }
}
