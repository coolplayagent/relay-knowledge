use super::*;

#[test]
fn admission_requires_metadata_and_markers_but_not_deferred_query_index() {
    let connection = Connection::open_in_memory().unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    connection
        .execute("DELETE FROM code_repository_schema_migrations", [])
        .unwrap();
    for marker in [
        SEARCH_OWNER_V2_MIGRATION,
        SEARCH_ORPHAN_GC_PHASE_MIGRATION,
        REFERENCE_SEARCH_GROUP_V2_MIGRATION,
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_schema_migrations(name,applied_at_ms) VALUES(?1,0)",
                [marker],
            )
            .unwrap();
    }
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    connection
        .execute(
            "INSERT INTO code_repository_schema_migrations(name,applied_at_ms) VALUES(?1,0)",
            [REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION],
        )
        .unwrap();
    connection
        .execute("DROP TRIGGER code_repository_java_namespace_insert", [])
        .unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    assert!(code_schema_capability_markers_are_current(&connection).unwrap());
    connection
        .execute(
            "ALTER TABLE code_repository_feature_flags DROP COLUMN metadata_json",
            [],
        )
        .unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
}

#[test]
fn warm_open_upgrades_legacy_namespace_columns_without_backfill_or_deferred_indexes() {
    let connection = Connection::open_in_memory().unwrap();
    super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF;
        INSERT INTO code_repository_files(repository_id,source_scope,file_id,path,language_id,blob_hash,byte_len,line_count,parse_status)
        VALUES('repo','old','file','App.java','java','blob',1,1,'parsed');
        DROP TRIGGER code_repository_java_namespace_insert;
        DROP TRIGGER code_repository_java_namespace_update;
        DROP TRIGGER code_repository_java_namespace_delete;
        DROP TABLE code_repository_java_types;
        DROP TABLE code_repository_java_namespaces;
        ALTER TABLE code_repository_files DROP COLUMN java_namespace_json;").unwrap();
    // All pre-existing warm-open markers are retained, as in the previous release.
    assert!(!super::super::marker::schema_initialization_is_current(&connection).unwrap());
    super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    assert!(code_schema_capability_markers_are_current(&connection).unwrap());
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_namespaces",
                [],
                |row| row.get::<_, usize>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('code_repository_java_namespace_completeness_lookup','code_repository_java_type_lookup')", [], |row| row.get::<_, usize>(0)).unwrap(), 0);
    assert!(
        connection
            .query_row(
                "SELECT java_namespace_json FROM code_repository_files WHERE path='App.java'",
                [],
                |row| row.get::<_, Option<String>>(0)
            )
            .unwrap()
            .is_none()
    );
    connection
        .execute(
            "UPDATE code_repository_files SET java_namespace_json=?1 WHERE path='App.java'",
            [r#"{"package":"demo","top_level_types":["App"],"complete":true,"source_set":{"kind":"repository"}}"#],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_java_types WHERE type_name='App'",
                [],
                |row| row.get::<_, usize>(0)
            )
            .unwrap(),
        1
    );
}
