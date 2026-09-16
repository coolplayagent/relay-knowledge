use super::*;
use crate::code::test_fixtures::TempGitRepo;

#[test]
fn source_io_directory_iterator_failure_discards_the_partial_listing() {
    let repo = TempGitRepo::create("io-enumeration");
    repo.write("src/A.java", "class A {}");
    repo.write("src/B.java", "class B {}");
    let path = repo.path.join("src");
    let _guard = test_fault::inject(
        path.clone(),
        CodePathIoOperation::ReadDirectory,
        2,
        io::Error::new(io::ErrorKind::PermissionDenied, "iterator fault"),
    );
    assert_eq!(
        read_directory(&path).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(read_directory(&path).unwrap().len(), 2);
}

#[test]
fn source_io_source_faults_are_path_and_operation_scoped_and_guards_restore_access() {
    let repo = TempGitRepo::create("io-boundary");
    repo.write("src/A.java", "class A {}");
    let path = repo.path.join("src/A.java");
    {
        let _guard = test_fault::inject(
            path.clone(),
            CodePathIoOperation::Read,
            0,
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert!(symlink_metadata(&path).is_ok());
        assert_eq!(
            read_file(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
    assert!(!read_file(&path).unwrap().is_empty());
}
