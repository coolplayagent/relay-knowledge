use super::*;

#[test]
fn persists_metadata_with_indexed_usage_identity() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    let transaction = connection.transaction().unwrap();
    let record = test_support::record("flag", "key", "config_key");
    insert_records(&transaction, std::slice::from_ref(&record)).unwrap();
    let json: String = transaction
        .query_row(
            "SELECT metadata_json FROM code_repository_feature_flags",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<crate::domain::CodeFeatureFlagMetadata>(&json).unwrap(),
        record.metadata
    );
    transaction.commit().unwrap();
}

#[test]
fn warm_open_upgrades_legacy_metadata_column() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-config-metadata-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&path).unwrap();
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "ALTER TABLE code_repository_feature_flags DROP COLUMN metadata_json",
                [],
            )
            .unwrap();
    }
    let store = crate::storage::SqliteGraphStore::open(&path).unwrap();
    let connection = store.connection.lock().unwrap();
    let default: String = connection.query_row("SELECT dflt_value FROM pragma_table_info('code_repository_feature_flags') WHERE name = 'metadata_json'", [], |r| r.get(0)).unwrap();
    assert_eq!(default, "'{}'");
    drop(connection);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
