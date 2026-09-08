use super::*;

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
    assert!(
        managed_database_validation(Path::new("E:/custom/relay-knowledge.sqlite"))
            .unwrap()
            .is_none()
    );
    validate_new_database_access(Path::new("E:/custom/relay-knowledge.sqlite")).unwrap();
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

#[tokio::test]
async fn fresh_managed_database_open_requires_successful_current_validation() {
    tokio::task::spawn_blocking(|| {
        let missing = Path::new("D:/relay-knowledge/users/S-1-5-21-4294967295-4294967295-4294967295-4294967295/data/review-missing.sqlite");
        assert!(validate_new_database_access(missing).is_err());
        assert!(!missing.exists(), "existing-only checks must never create a database");
    }).await.unwrap();
}
