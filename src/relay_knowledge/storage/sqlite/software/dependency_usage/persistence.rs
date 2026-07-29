use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{GraphVersion, RepositoryCodeRange, SoftwareDependencyUsage, SoftwareGlobalRequest},
    storage::StorageError,
};

pub(in crate::storage::sqlite::software) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM software_dependency_usages WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite::software) fn insert_usage(
    connection: &Connection,
    usage: &SoftwareDependencyUsage,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_dependency_usages (
            usage_id, component_id, repository_id, source_scope, ecosystem, package_name,
            language_id, module, target_hint, resolution_state, evidence_path,
            evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        params![
            usage.usage_id,
            usage.component_id,
            usage.repository_id,
            usage.source_scope,
            usage.ecosystem,
            usage.package_name,
            usage.language_id,
            usage.module,
            usage.target_hint,
            usage.resolution_state,
            usage.evidence_path,
            usage.evidence_line_range.start,
            usage.evidence_line_range.end,
            usage.confidence_basis_points,
            usage.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite::software) fn usages_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareDependencyUsage>, StorageError> {
    let path_filter =
        super::super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
    let query = format!(
        "
        SELECT usage_id, component_id, repository_id, source_scope, ecosystem, package_name,
               language_id, module, target_hint, resolution_state, evidence_path,
               evidence_line_start, evidence_line_end, confidence_basis_points,
               created_graph_version
        FROM software_dependency_usages
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY ecosystem ASC, package_name ASC, evidence_path ASC, evidence_line_start ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), usage_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn import_evidence(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<ImportEvidence>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT imports.repository_id, imports.source_scope, files.language_id,
               imports.module, imports.target_hint, imports.resolution_state,
               imports.path, imports.line_start, imports.line_end,
               imports.confidence_basis_points
        FROM code_repository_imports imports
        JOIN code_repository_files files
          ON files.source_scope = imports.source_scope
         AND files.path = imports.path
        WHERE imports.source_scope = ?1
        ORDER BY files.language_id ASC, imports.module ASC, imports.path ASC, imports.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ImportEvidence {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            language_id: row.get(2)?,
            module: row.get(3)?,
            target_hint: row.get(4)?,
            resolution_state: row.get(5)?,
            evidence_path: row.get(6)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
            confidence_basis_points: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareDependencyUsage> {
    Ok(SoftwareDependencyUsage {
        usage_id: row.get(0)?,
        component_id: row.get(1)?,
        repository_id: row.get(2)?,
        source_scope: row.get(3)?,
        ecosystem: row.get(4)?,
        package_name: row.get(5)?,
        language_id: row.get(6)?,
        module: row.get(7)?,
        target_hint: row.get(8)?,
        resolution_state: row.get(9)?,
        evidence_path: row.get(10)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(11)?,
            end: row.get(12)?,
        },
        confidence_basis_points: row.get(13)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(14)?),
    })
}

pub(super) struct ImportEvidence {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) language_id: String,
    pub(super) module: String,
    pub(super) target_hint: Option<String>,
    pub(super) resolution_state: String,
    pub(super) evidence_path: String,
    pub(super) evidence_line_range: RepositoryCodeRange,
    pub(super) confidence_basis_points: u16,
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
