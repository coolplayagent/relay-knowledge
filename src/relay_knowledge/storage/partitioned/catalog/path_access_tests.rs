use super::*;

#[test]
fn fresh_catalog_reads_and_writes_reject_invalid_managed_paths_before_sqlite() {
    let path = Path::new("D:/relay-knowledge/users/S-1-invalid/data/relay-knowledge.sqlite");
    assert!(
        open_catalog_connection(path)
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        open_catalog_readonly_connection(path)
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        upsert_catalog_repository(path, "repo", "shard.sqlite")
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        catalog_repository_for_scope(path, "scope")
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(!path.exists());
}
