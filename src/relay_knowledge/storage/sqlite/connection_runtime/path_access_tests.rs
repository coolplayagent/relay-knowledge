use super::*;

static SECURITY_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn fresh_database_policy_recovers_control_and_shard_accounts_without_filesystem_work() {
    for path in [
        "D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data/relay-knowledge.sqlite",
        "D:/relay-knowledge/users/S-1-5-21-1-2-3-1001/data/backends/repositories/repo/code.sqlite",
    ] {
        assert!(
            managed_database_validation(Path::new(path))
                .unwrap()
                .is_some()
        );
    }
    let custom = Path::new("E:/custom/relay-knowledge.sqlite");
    if managed_database_validation(custom).unwrap().is_none() {
        validate_new_database_access(custom).unwrap();
    }
    assert!(
        validate_new_database_access(Path::new(
            "D:/relay-knowledge/users/S-1-invalid/data/relay-knowledge.sqlite"
        ))
        .unwrap_err()
        .to_string()
        .contains("invalid account SID")
    );
    assert!(
        validate_new_database_access(Path::new(&"x".repeat(4097)))
            .unwrap_err()
            .to_string()
            .contains("4096")
    );
}

#[test]
fn fresh_managed_database_open_checks_policy_with_or_without_an_ambient_runtime() {
    let _serial = SECURITY_TEST.lock().unwrap();
    let missing = Path::new(
        "D:/relay-knowledge/users/S-1-5-21-4294967295-4294967295-4294967295-4294967295/data/review-missing.sqlite",
    );
    let error = validate_new_database_access(missing).unwrap_err();
    assert!(!error.to_string().contains("no reactor"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(async { validate_new_database_access(missing) })
        .unwrap_err();
    assert!(!error.to_string().contains("no reactor"));
    assert!(
        !missing.exists(),
        "existing-only checks must never create a database"
    );
}

#[test]
fn occupied_security_worker_applies_backpressure_without_opening_another_thread() {
    let _serial = SECURITY_TEST.lock().unwrap();
    let _occupied = SECURITY_WORKER.lock().unwrap();
    let path = Path::new("D:/relay-knowledge/users/S-1-5-18/data/relay-knowledge.sqlite");
    assert!(
        matches!(validate_new_database_access(path), Err(StorageError::Busy(message)) if message.contains("security worker"))
    );
    let custom = Path::new("E:/custom/relay-knowledge.sqlite");
    if managed_database_validation(custom).unwrap().is_none() {
        validate_new_database_access(custom).unwrap();
    } else {
        assert!(matches!(
            validate_new_database_access(custom),
            Err(StorageError::Busy(_))
        ));
    }
}
