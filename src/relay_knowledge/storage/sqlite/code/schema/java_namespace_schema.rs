//! Per-file Java namespace projections, maintained by the original fact write.
use crate::storage::StorageError;
use rusqlite::Connection;

pub(super) fn initialize(connection: &Connection) -> Result<(), StorageError> {
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_files",
        "java_namespace_json",
        "TEXT",
    )?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS code_repository_java_namespaces (
            source_scope TEXT NOT NULL, path TEXT NOT NULL,
            package TEXT NOT NULL, complete INTEGER NOT NULL,
            PRIMARY KEY(source_scope, path),
            FOREIGN KEY(source_scope,path) REFERENCES code_repository_files(source_scope,path) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS code_repository_java_types (
            source_scope TEXT NOT NULL, path TEXT NOT NULL,
            package TEXT NOT NULL, type_name TEXT NOT NULL,
            PRIMARY KEY(source_scope,path,type_name),
            FOREIGN KEY(source_scope,path) REFERENCES code_repository_files(source_scope,path) ON DELETE CASCADE
        );
        CREATE TRIGGER IF NOT EXISTS code_repository_java_namespace_delete
        AFTER DELETE ON code_repository_files BEGIN
            DELETE FROM code_repository_java_types WHERE source_scope=OLD.source_scope AND path=OLD.path;
            DELETE FROM code_repository_java_namespaces WHERE source_scope=OLD.source_scope AND path=OLD.path;
        END;",
    )?;
    // Coarse legacy owners are not reinterpreted as complete file evidence.
    if !super::migrations::table_has_columns(
        connection,
        "code_repository_files",
        &["source_scope", "path", "language_id", "parse_status"],
    )? {
        return Ok(());
    }
    // The payload and list bounds are enforced even for imported legacy facts.
    // Invalid/absent evidence creates an Unknown row rather than proving absence.
    let evidence = "CASE WHEN length(NEW.java_namespace_json)<=65536 AND json_valid(NEW.java_namespace_json) THEN NEW.java_namespace_json ELSE '{}' END";
    let complete = format!(
        "NEW.parse_status='parsed' AND json_extract({evidence},'$.complete')=1
         AND json_type({evidence},'$.package')='text'
         AND length(json_extract({evidence},'$.package'))<=4096
         AND json_type({evidence},'$.top_level_types')='array'
         AND json_array_length({evidence},'$.top_level_types')<=1024
         AND NOT EXISTS(SELECT 1 FROM json_each({evidence},'$.top_level_types')
             WHERE type<>'text' OR length(value)>1024 OR length(value)=0)
         AND (1+json_array_length({evidence},'$.top_level_types')) *
             (length(CAST(NEW.source_scope AS BLOB))+length(CAST(NEW.path AS BLOB))
              +length(CAST(json_extract({evidence},'$.package') AS BLOB))+128)
             +COALESCE((SELECT sum(length(CAST(value AS BLOB))) FROM json_each({evidence},'$.top_level_types')),0)<=65536"
    );
    for (name, event) in [
        ("insert", "INSERT"),
        (
            "update",
            "UPDATE OF java_namespace_json,language_id,parse_status,source_scope,path",
        ),
    ] {
        let old_owner = if name == "update" {
            "DELETE FROM code_repository_java_types WHERE source_scope=OLD.source_scope AND path=OLD.path;
             DELETE FROM code_repository_java_namespaces WHERE source_scope=OLD.source_scope AND path=OLD.path;"
        } else {
            ""
        };
        connection.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS code_repository_java_namespace_{name}
             AFTER {event} ON code_repository_files BEGIN
             {old_owner}
             DELETE FROM code_repository_java_types WHERE source_scope=NEW.source_scope AND path=NEW.path;
             DELETE FROM code_repository_java_namespaces WHERE source_scope=NEW.source_scope AND path=NEW.path;
             INSERT INTO code_repository_java_namespaces(source_scope,path,package,complete)
             SELECT NEW.source_scope,NEW.path,CASE WHEN {complete} THEN json_extract({evidence},'$.package') ELSE '' END,
                    CASE WHEN {complete} THEN 1 ELSE 0 END WHERE NEW.language_id='java';
             INSERT OR IGNORE INTO code_repository_java_types(source_scope,path,package,type_name)
             SELECT NEW.source_scope,NEW.path,namespace.package,types.value
             FROM code_repository_java_namespaces namespace
             CROSS JOIN json_each({evidence},'$.top_level_types') types
             WHERE namespace.source_scope=NEW.source_scope AND namespace.path=NEW.path AND namespace.complete=1;
             END;"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "java_namespace_schema_tests.rs"]
mod tests;
