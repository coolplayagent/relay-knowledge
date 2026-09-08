use super::*;

#[test]
fn admission_requires_metadata_key_index_and_all_existing_migration_markers() {
    let connection = Connection::open_in_memory().unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    connection.execute_batch("CREATE TABLE code_repository_schema_migrations(name TEXT PRIMARY KEY);
        CREATE TABLE code_repository_feature_flags(source_scope TEXT, source_kind TEXT, source_key TEXT, metadata_json TEXT);").unwrap();
    for marker in [
        SEARCH_OWNER_V2_MIGRATION,
        SEARCH_ORPHAN_GC_PHASE_MIGRATION,
        REFERENCE_SEARCH_GROUP_V2_MIGRATION,
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_schema_migrations VALUES(?1)",
                [marker],
            )
            .unwrap();
    }
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    connection.execute_batch("CREATE INDEX code_repository_feature_flags_source_key ON code_repository_feature_flags(source_scope, source_kind, source_key)").unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
    connection
        .execute(
            "INSERT INTO code_repository_schema_migrations VALUES(?1)",
            [REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION],
        )
        .unwrap();
    assert!(code_schema_capability_markers_are_current(&connection).unwrap());
    connection
        .execute(
            "ALTER TABLE code_repository_feature_flags DROP COLUMN metadata_json",
            [],
        )
        .unwrap();
    assert!(!code_schema_capability_markers_are_current(&connection).unwrap());
}
