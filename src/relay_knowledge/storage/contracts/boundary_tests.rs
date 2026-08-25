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
    assert_eq!(
        StorageError::CapacityExceeded("queue is full".to_owned()).to_string(),
        "storage capacity exceeded: queue is full"
    );
    assert_eq!(
        StorageError::DurableStagingRequired("cross-scope publication".to_owned()).to_string(),
        "durable staging required: cross-scope publication"
    );
    assert_eq!(
        StorageError::DurableStagingPending {
            completed_steps: 7,
            max_steps: 19,
        }
        .to_string(),
        "durable staging pending after step 7 of at most 19"
    );
    assert_eq!(
        StorageError::DurableFinalizationRequired {
            checkpoint_state: "finalizing:build_query_indexes".to_owned(),
        }
        .to_string(),
        "durable incremental delta committed; finalization must resume from 'finalizing:build_query_indexes'"
    );
    assert_eq!(
        StorageError::Invariant("checkpoint cursor regressed".to_owned()).to_string(),
        "storage invariant failed: checkpoint cursor regressed"
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
