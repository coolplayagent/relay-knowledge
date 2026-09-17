//! Transactional inverted identities derived from bounded configuration metadata.
use super::*;

pub(super) fn binding_schema_present(connection: &Connection) -> Result<bool, StorageError> {
    let columns = super::super::super::schema::introspection::table_has_primary_key_columns(
        connection,
        "code_repository_config_bindings",
        &["source_scope", "binding", "usage_id"],
    )?;
    let triggers: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND tbl_name='code_repository_feature_flags' AND name IN ('code_config_bindings_insert','code_config_bindings_update','code_config_bindings_delete')", [], |row| row.get(0))?;
    let usage_index = super::super::super::schema::introspection::index_has_columns(
        connection,
        "code_repository_config_bindings_usage",
        &["source_scope", "usage_id"],
    )?;
    Ok(columns && triggers == 3 && usage_index)
}

pub(in crate::storage::sqlite::code) fn initialize_config_bindings(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS code_repository_config_bindings (
        source_scope TEXT NOT NULL, binding TEXT NOT NULL, usage_id TEXT NOT NULL,
        PRIMARY KEY(source_scope,binding,usage_id)
    );
    CREATE INDEX IF NOT EXISTS code_repository_config_bindings_usage ON code_repository_config_bindings(source_scope,usage_id);")?;
    let insert = "INSERT OR IGNORE INTO code_repository_config_bindings(source_scope,binding,usage_id)
        SELECT new.source_scope,value,new.usage_id FROM (
            SELECT value FROM json_each(new.metadata_json,'$.bindings')
            UNION SELECT json_extract(new.metadata_json,'$.reference')
            UNION SELECT json_extract(new.metadata_json,'$.same_package_reference')
            UNION SELECT json_extract(new.metadata_json,'$.lexical_field_reference')
            UNION SELECT value FROM json_each(new.metadata_json,'$.lexical_getter_references')
            UNION SELECT value FROM json_each(new.metadata_json,'$.same_package_parents')
            UNION SELECT json_extract(value,'$.reference') FROM json_each(new.metadata_json,'$.string_parts')
        ) WHERE typeof(value)='text'";
    let insert = insert.replace(
        "new.metadata_json",
        "CASE WHEN json_valid(new.metadata_json) THEN new.metadata_json ELSE '{}' END",
    );
    connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS code_config_bindings_insert AFTER INSERT ON code_repository_feature_flags BEGIN
        DELETE FROM code_repository_config_bindings WHERE source_scope=new.source_scope AND usage_id=new.usage_id;
        {insert}; END;
    CREATE TRIGGER IF NOT EXISTS code_config_bindings_delete AFTER DELETE ON code_repository_feature_flags BEGIN
        DELETE FROM code_repository_config_bindings WHERE source_scope=old.source_scope AND usage_id=old.usage_id; END;
    CREATE TRIGGER IF NOT EXISTS code_config_bindings_update AFTER UPDATE OF source_scope,usage_id,metadata_json ON code_repository_feature_flags BEGIN
        DELETE FROM code_repository_config_bindings WHERE source_scope=old.source_scope AND usage_id=old.usage_id;
        {insert}; END;"))?;
    Ok(())
}
