use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{GraphVersion, SoftwareFile, SoftwareFileInput, SoftwareGlobalRequest},
    project::{KNOWLEDGE_MAP_RELATIVE_PATH, LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH},
    storage::StorageError,
};

use super::file_role::file_role;

const FILE_PROJECTION_PAGE_SIZE: usize = 512;
const SOFTWARE_FILE_INSERT_SQL: &str = "
    INSERT OR REPLACE INTO software_files (
        software_file_id, repository_id, source_scope, path, language_id, file_role,
        parse_status, created_graph_version
    )
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
";

pub(in crate::storage::sqlite::software) fn materialize_files(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<usize, StorageError> {
    let mut insert_statement = connection.prepare(SOFTWARE_FILE_INSERT_SQL)?;
    let mut after_path = None::<String>;
    let mut count = 0;
    loop {
        let files = software_file_page(
            connection,
            source_scope,
            graph_version,
            FILE_PROJECTION_PAGE_SIZE,
            after_path.as_deref(),
        )?;
        if files.is_empty() {
            break;
        }
        for file in &files {
            insert_file(&mut insert_statement, file)?;
        }
        let page_len = files.len();
        after_path = files.last().map(|file| file.path.clone());
        count += page_len;
    }

    Ok(count)
}

fn software_file_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    after_path: Option<&str>,
) -> Result<Vec<SoftwareFile>, StorageError> {
    let (query, values) = match after_path {
        Some(path) => (
            "
            SELECT files.repository_id, files.source_scope, files.path, files.language_id,
                   files.parse_status,
                   EXISTS (
                       SELECT 1
                       FROM code_repository_symbols refs
                       JOIN code_repository_symbols topics
                         ON topics.repository_id = refs.repository_id
                        AND topics.source_scope = refs.source_scope
                        AND topics.path = refs.path
                        AND topics.line_start = refs.line_start
                        AND topics.kind = 'knowledge_map_topic_shard_topic'
                       JOIN code_repository_symbols root_identity
                         ON root_identity.repository_id = refs.repository_id
                        AND root_identity.source_scope = refs.source_scope
                        AND root_identity.path = refs.path
                        AND root_identity.line_start = refs.line_start
                        AND root_identity.kind = 'knowledge_map_topic_shard_identity'
                       JOIN code_repository_symbols shards
                         ON shards.repository_id = refs.repository_id
                        AND shards.source_scope = refs.source_scope
                        AND shards.path = refs.name
                        AND shards.name = topics.name
                        AND shards.kind = 'knowledge_map_topic_shard'
                       JOIN code_repository_symbols shard_identity
                         ON shard_identity.repository_id = shards.repository_id
                        AND shard_identity.source_scope = shards.source_scope
                        AND shard_identity.path = shards.path
                        AND shard_identity.line_start = shards.line_start
                        AND shard_identity.kind = 'knowledge_map_topic_shard_identity'
                        AND shard_identity.name = root_identity.name
                       WHERE refs.source_scope = files.source_scope
                         AND refs.repository_id = files.repository_id
                         AND refs.path IN (?3, ?4)
                         AND refs.kind = 'knowledge_map_topic_shard_ref'
                         AND refs.name = files.path
                   ) AS authorized_topic_shard
            FROM code_repository_files files
            WHERE files.source_scope = ?1 AND files.path > ?2
            ORDER BY files.path ASC
            LIMIT ?5
            ",
            vec![
                Value::Text(source_scope.to_owned()),
                Value::Text(path.to_owned()),
                Value::Text(KNOWLEDGE_MAP_RELATIVE_PATH.to_owned()),
                Value::Text(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH.to_owned()),
                Value::Integer(limit as i64),
            ],
        ),
        None => (
            "
            SELECT files.repository_id, files.source_scope, files.path, files.language_id,
                   files.parse_status,
                   EXISTS (
                       SELECT 1
                       FROM code_repository_symbols refs
                       JOIN code_repository_symbols topics
                         ON topics.repository_id = refs.repository_id
                        AND topics.source_scope = refs.source_scope
                        AND topics.path = refs.path
                        AND topics.line_start = refs.line_start
                        AND topics.kind = 'knowledge_map_topic_shard_topic'
                       JOIN code_repository_symbols root_identity
                         ON root_identity.repository_id = refs.repository_id
                        AND root_identity.source_scope = refs.source_scope
                        AND root_identity.path = refs.path
                        AND root_identity.line_start = refs.line_start
                        AND root_identity.kind = 'knowledge_map_topic_shard_identity'
                       JOIN code_repository_symbols shards
                         ON shards.repository_id = refs.repository_id
                        AND shards.source_scope = refs.source_scope
                        AND shards.path = refs.name
                        AND shards.name = topics.name
                        AND shards.kind = 'knowledge_map_topic_shard'
                       JOIN code_repository_symbols shard_identity
                         ON shard_identity.repository_id = shards.repository_id
                        AND shard_identity.source_scope = shards.source_scope
                        AND shard_identity.path = shards.path
                        AND shard_identity.line_start = shards.line_start
                        AND shard_identity.kind = 'knowledge_map_topic_shard_identity'
                        AND shard_identity.name = root_identity.name
                       WHERE refs.source_scope = files.source_scope
                         AND refs.repository_id = files.repository_id
                         AND refs.path IN (?2, ?3)
                         AND refs.kind = 'knowledge_map_topic_shard_ref'
                         AND refs.name = files.path
                   ) AS authorized_topic_shard
            FROM code_repository_files files
            WHERE files.source_scope = ?1
            ORDER BY files.path ASC
            LIMIT ?4
            ",
            vec![
                Value::Text(source_scope.to_owned()),
                Value::Text(KNOWLEDGE_MAP_RELATIVE_PATH.to_owned()),
                Value::Text(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH.to_owned()),
                Value::Integer(limit as i64),
            ],
        ),
    };
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        let path = row.get::<_, String>(2)?;
        let language_id = row.get::<_, String>(3)?;
        let authorized_topic_shard = row.get::<_, bool>(5)?;
        Ok(SoftwareFileInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            file_role: file_role(&path, &language_id, authorized_topic_shard).to_owned(),
            path,
            language_id,
            parse_status: row.get(4)?,
            created_graph_version: graph_version,
        })
    })?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareFile::new(input).map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

fn insert_file(
    statement: &mut rusqlite::Statement<'_>,
    file: &SoftwareFile,
) -> Result<(), StorageError> {
    statement.execute(params![
        file.software_file_id,
        file.repository_id,
        file.source_scope,
        file.path,
        file.language_id,
        file.file_role,
        file.parse_status,
        file.created_graph_version.get(),
    ])?;

    Ok(())
}

pub(in crate::storage::sqlite::software) fn files_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareFile>, StorageError> {
    let path_filter =
        super::super::path_filter_sql_for_column("path", &request.repository.path_filters);
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
    let query = format!(
        "
        SELECT software_file_id, repository_id, source_scope, path, language_id, file_role,
               parse_status, created_graph_version
        FROM software_files
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE file_role
                WHEN 'dependency_manifest' THEN 0
                WHEN 'build_manifest' THEN 1
                WHEN 'source' THEN 2
                WHEN 'documentation' THEN 3
                WHEN 'configuration' THEN 4
                WHEN 'deployment' THEN 5
                WHEN 'test' THEN 6
                WHEN 'template' THEN 7
                WHEN 'knowledge_map_manifest' THEN 8
                WHEN 'knowledge_map_topic_shard' THEN 9
                ELSE 9
            END ASC,
            CASE
                WHEN path = 'Cargo.toml' OR path LIKE '%/Cargo.toml' THEN 0
                WHEN path = 'package.json' OR path LIKE '%/package.json' THEN 1
                WHEN path = 'pyproject.toml' OR path LIKE '%/pyproject.toml' THEN 2
                WHEN path = 'go.mod' OR path LIKE '%/go.mod' THEN 3
                WHEN path = 'pom.xml' OR path LIKE '%/pom.xml' THEN 4
                WHEN path = 'build.gradle' OR path LIKE '%/build.gradle'
                  OR path = 'build.gradle.kts' OR path LIKE '%/build.gradle.kts' THEN 5
                WHEN path = 'CMakeLists.txt' OR path LIKE '%/CMakeLists.txt' THEN 6
                WHEN path = 'Makefile' OR path LIKE '%/Makefile' THEN 7
                WHEN path = 'Cargo.lock' OR path LIKE '%/Cargo.lock'
                  OR path = 'package-lock.json' OR path LIKE '%/package-lock.json'
                  OR path = 'go.sum' OR path LIKE '%/go.sum'
                  OR path = 'uv.lock' OR path LIKE '%/uv.lock'
                  OR path = 'gradle.lockfile' OR path LIKE '%/gradle.lockfile' THEN 20
                ELSE 10
            END ASC,
            path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), file_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn file_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareFile> {
    Ok(SoftwareFile {
        software_file_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        path: row.get(3)?,
        language_id: row.get(4)?,
        file_role: row.get(5)?,
        parse_status: row.get(6)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(7)?),
    })
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
