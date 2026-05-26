use rusqlite::{Transaction, params};

use crate::{
    domain::{CodeDependencyRecord, CodeIndexBatch},
    storage::StorageError,
};

pub(super) fn insert_dependencies(
    transaction: &Transaction<'_>,
    batch: &CodeIndexBatch,
) -> Result<(), StorageError> {
    insert_dependency_records(transaction, &batch.dependencies)
}

pub(in crate::storage::sqlite::code) fn insert_dependency_records(
    transaction: &Transaction<'_>,
    dependencies: &[CodeDependencyRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_dependencies (
            repository_id, source_scope, dependency_id, file_id, path, language_id,
            ecosystem, package_name, requirement, resolved_version, dependency_group,
            source_kind, is_lockfile, line_start, line_end, excerpt
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    let mut search_documents = super::super::SearchDocumentInserter::new(transaction)?;
    for dependency in dependencies {
        statement.execute(params![
            dependency.repository_id,
            dependency.source_scope,
            dependency.dependency_id,
            dependency.file_id,
            dependency.path,
            dependency.language_id,
            dependency.ecosystem,
            dependency.package_name,
            dependency.requirement,
            dependency.resolved_version,
            dependency.dependency_group,
            dependency.source_kind,
            dependency.is_lockfile,
            dependency.line_range.start,
            dependency.line_range.end,
            dependency.excerpt,
        ])?;
        search_documents.insert(
            &dependency.source_scope,
            "dependency",
            &dependency.dependency_id,
            &dependency.path,
            &dependency.language_id,
            [
                dependency.ecosystem.as_str(),
                dependency.package_name.as_str(),
                dependency.requirement.as_deref().unwrap_or_default(),
                dependency.resolved_version.as_deref().unwrap_or_default(),
                dependency.dependency_group.as_str(),
                dependency.source_kind.as_str(),
                dependency.excerpt.as_str(),
                dependency.path.as_str(),
            ],
        )?;
    }

    Ok(())
}
