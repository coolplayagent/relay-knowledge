use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{GraphVersion, SoftwareFile, SoftwareFileInput, SoftwareGlobalRequest},
    storage::StorageError,
};

use super::file_role::file_role;

const FILE_PROJECTION_PAGE_SIZE: usize = 512;

pub(in crate::storage::sqlite::software) fn materialize_files(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<usize, StorageError> {
    let mut offset = 0;
    let mut count = 0;
    loop {
        let files = software_file_page(
            connection,
            source_scope,
            graph_version,
            FILE_PROJECTION_PAGE_SIZE,
            offset,
        )?;
        if files.is_empty() {
            break;
        }
        for file in &files {
            insert_file(connection, file)?;
        }
        let page_len = files.len();
        count += page_len;
        offset += page_len;
    }

    Ok(count)
}

fn software_file_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareFile>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, path, language_id, parse_status
        FROM code_repository_files
        WHERE source_scope = ?1
        ORDER BY path ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        let path = row.get::<_, String>(2)?;
        let language_id = row.get::<_, String>(3)?;
        Ok(SoftwareFileInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            file_role: file_role(&path, &language_id).to_owned(),
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

fn insert_file(connection: &Connection, file: &SoftwareFile) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_files (
            software_file_id, repository_id, source_scope, path, language_id, file_role,
            parse_status, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            file.software_file_id,
            file.repository_id,
            file.source_scope,
            file.path,
            file.language_id,
            file.file_role,
            file.parse_status,
            file.created_graph_version.get(),
        ],
    )?;

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
                WHEN 'knowledge_map' THEN 8
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
