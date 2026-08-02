use super::*;

#[test]
fn storage_errors_preserve_boundary_messages() {
    let io = StorageError::from(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "readonly",
    ));
    let sqlite = StorageError::from(rusqlite::Error::InvalidQuery);

    assert!(io.to_string().contains("storage I/O failed: readonly"));
    assert_eq!(
        sqlite.to_string(),
        "sqlite operation failed: Query is not read-only"
    );
    assert_eq!(
        StorageError::LockPoisoned.to_string(),
        "sqlite connection lock was poisoned"
    );
    assert_eq!(
        StorageError::InvalidInput("missing graph version".to_owned()).to_string(),
        "invalid storage input: missing graph version"
    );
}

#[tokio::test]
async fn join_errors_map_to_storage_worker_failures() {
    let join_error = tokio::spawn(async { panic!("storage worker panic") })
        .await
        .expect_err("worker should panic");
    let error = StorageError::from(join_error);

    assert!(error.to_string().contains("storage worker failed"));
}
