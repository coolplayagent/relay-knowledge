//! Cost of file-triggered Java namespace rows, without loading source text.

// These expressions run against the persisted source file alias. Both correlated
// reads seek its exact (scope,path) primary-key prefix. Missing legacy projection
// still costs the Unknown namespace row that the destination trigger creates.
pub(super) const COPY_ROWS: &str = "CASE WHEN source.language_id='java' THEN
    1+(SELECT count(*) FROM code_repository_java_types java_type
       WHERE java_type.source_scope=source.source_scope AND java_type.path=source.path) ELSE 0 END";

pub(super) const COPY_BYTES: &str = "CASE WHEN source.language_id='java' THEN
    length(CAST(source.source_scope AS BLOB))+length(CAST(source.path AS BLOB))+128
    +COALESCE((SELECT length(CAST(package AS BLOB)) FROM code_repository_java_namespaces namespace
       WHERE namespace.source_scope=source.source_scope AND namespace.path=source.path),0)
    +COALESCE((SELECT sum(length(CAST(source_scope AS BLOB))+length(CAST(path AS BLOB))
       +length(CAST(package AS BLOB))+length(CAST(type_name AS BLOB))+128)
       FROM code_repository_java_types java_type
       WHERE java_type.source_scope=source.source_scope AND java_type.path=source.path),0) ELSE 0 END";

#[cfg(test)]
#[path = "java_projection_tests.rs"]
mod tests;
