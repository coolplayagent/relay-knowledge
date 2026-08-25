//! Rebuilds finalized reference, import, and call search documents transactionally.

use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use crate::{domain::CodeIndexResourceBudget, storage::StorageError};

use super::super::super::{
    SearchDocumentInserter,
    search::{ReferenceSearchGroupStorage, delete_search_documents_for_kind},
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "unfenced_tests.rs"]
mod unfenced_tests;

mod grouped;
mod legacy;

pub(in crate::storage::sqlite::code::batch) use grouped::{
    ReferenceSearchAdvance, advance_reference_search_progress, initialize_reference_search_progress,
};

pub(super) fn rebuild_reference_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<(), StorageError> {
    // Reserve the manifest owner plus the caller's checkpoint CAS.
    let document_limit = resource_budget.max_rows_per_batch.saturating_sub(2) / 3;
    if document_limit == 0 || expected_reference_count > document_limit {
        return Err(StorageError::CapacityExceeded(format!(
            "unfenced reference-search rebuild for scope '{source_scope}' exceeds one bounded writer quantum; use fenced staged finalization"
        )));
    }
    let actual_reference_count = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM code_repository_references
             WHERE source_scope = ?1 ORDER BY reference_id LIMIT ?2
         )",
        params![source_scope, document_limit + 1],
        |row| row.get::<_, usize>(0),
    )?;
    if actual_reference_count != expected_reference_count {
        return Err(StorageError::Invariant(format!(
            "unfenced reference-search facts for scope '{source_scope}' do not match their frozen count"
        )));
    }
    let existing_owner_count = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM code_repository_search_metadata
             WHERE source_scope = ?1 AND document_kind = 'reference'
             LIMIT ?2
         )",
        params![source_scope, document_limit + 1],
        |row| row.get::<_, usize>(0),
    )?;
    let existing_group_count = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM code_repository_reference_search_groups
             WHERE source_scope = ?1 LIMIT ?2
         )",
        params![source_scope, document_limit + 1],
        |row| row.get::<_, usize>(0),
    )?;
    let existing_manifest = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_reference_search_manifests
             WHERE source_scope = ?1
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if existing_owner_count != 0 || existing_group_count != 0 || existing_manifest {
        return Err(StorageError::CapacityExceeded(format!(
            "unfenced reference-search rebuild for scope '{source_scope}' requires durable bounded cleanup; use fenced staged finalization"
        )));
    }
    let mut select = transaction.prepare(
        "
        SELECT reference.reference_id, reference.path, coalesce(file.language_id, ''),
               reference.name, reference.kind, coalesce(reference.target_hint, '')
        FROM code_repository_references reference
        LEFT JOIN code_repository_files file
          ON file.source_scope = reference.source_scope
         AND file.path = reference.path
        WHERE reference.source_scope = ?1
        ORDER BY reference.reference_id
        ",
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut groups = BTreeMap::new();
    let mut reference_count = 0usize;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let name = row.get::<_, String>(3)?;
        let kind = row.get::<_, String>(4)?;
        let target_hint = row.get::<_, String>(5)?;
        let key = (
            name.clone(),
            kind.clone(),
            path.clone(),
            target_hint.clone(),
        );
        let group = groups.entry(key).or_insert((
            record_id,
            path,
            language_id,
            name,
            kind,
            target_hint,
            0usize,
        ));
        group.6 = group.6.checked_add(1).ok_or_else(|| {
            StorageError::CapacityExceeded(
                "synchronous grouped reference-search count exceeds platform capacity".to_owned(),
            )
        })?;
        reference_count = reference_count.checked_add(1).ok_or_else(|| {
            StorageError::CapacityExceeded(
                "synchronous grouped reference-search count exceeds platform capacity".to_owned(),
            )
        })?;
    }
    drop(rows);
    drop(select);
    if reference_count != expected_reference_count {
        return Err(StorageError::CapacityExceeded(format!(
            "unfenced grouped reference-search projection for scope '{source_scope}' exceeds its frozen row or byte budget"
        )));
    }
    let group_count = groups.len();
    let group_bytes = groups.values().try_fold(
        source_scope.len().saturating_add(64),
        |bytes, (record_id, path, language_id, name, kind, target_hint, _)| {
            let persisted_bytes = ReferenceSearchGroupStorage {
                source_scope,
                group_id: record_id,
                name,
                reference_kind: kind,
                path,
                target_hint,
                language_id,
            }
            .persisted_byte_upper_bound()
            .ok_or_else(|| {
                StorageError::CapacityExceeded(
                    "unfenced grouped reference-search byte count exceeds platform capacity"
                        .to_owned(),
                )
            })?;
            bytes.checked_add(persisted_bytes).ok_or_else(|| {
                StorageError::CapacityExceeded(
                    "unfenced grouped reference-search byte count exceeds platform capacity"
                        .to_owned(),
                )
            })
        },
    )?;
    if group_bytes > resource_budget.max_bytes_per_batch {
        return Err(StorageError::CapacityExceeded(format!(
            "unfenced grouped reference-search projection for scope '{source_scope}' exceeds its byte budget"
        )));
    }
    let mutation_count = existing_owner_count
        .checked_mul(2)
        .and_then(|count| count.checked_add(existing_group_count))
        .and_then(|count| {
            group_count
                .checked_mul(3)
                .and_then(|groups| count.checked_add(groups))
        })
        .and_then(|count| count.checked_add(usize::from(existing_manifest)))
        .and_then(|count| count.checked_add(1))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| {
            StorageError::CapacityExceeded(
                "unfenced grouped reference-search mutation count exceeds platform capacity"
                    .to_owned(),
            )
        })?;
    if mutation_count > resource_budget.max_rows_per_batch {
        return Err(StorageError::CapacityExceeded(format!(
            "unfenced grouped reference-search projection for scope '{source_scope}' exceeds one bounded writer quantum"
        )));
    }
    delete_search_documents_for_kind(transaction, source_scope, "reference")?;
    transaction.execute(
        "DELETE FROM code_repository_reference_search_groups WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_reference_search_manifests WHERE source_scope = ?1",
        params![source_scope],
    )?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    for (_, (record_id, path, language_id, name, kind, target_hint, occurrence_count)) in groups {
        transaction.execute(
            "INSERT INTO code_repository_reference_search_groups (
                 source_scope, group_id, name, kind, path, target_hint,
                 language_id, occurrence_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                source_scope,
                record_id,
                name,
                kind,
                path,
                target_hint,
                language_id,
                occurrence_count,
            ],
        )?;
        inserter.insert(
            source_scope,
            "reference",
            &record_id,
            &path,
            &language_id,
            [
                name.as_str(),
                kind.as_str(),
                target_hint.as_str(),
                path.as_str(),
            ],
        )?;
    }
    inserter.finish()?;
    transaction.execute(
        "INSERT INTO code_repository_reference_search_manifests (
             source_scope, projection_version, reference_count, group_count
         ) VALUES (?1, 2, ?2, ?3)",
        params![source_scope, reference_count, group_count],
    )?;

    Ok(())
}

pub(super) fn rebuild_import_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    delete_search_documents_for_kind(transaction, source_scope, "import")?;
    let mut select = transaction.prepare(
        "
        SELECT import.import_id, import.path, coalesce(file.language_id, ''),
               import.module, coalesce(import.target_hint, '')
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        WHERE import.source_scope = ?1
        ",
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let module = row.get::<_, String>(3)?;
        let target_hint = row.get::<_, String>(4)?;
        inserter.insert(
            source_scope,
            "import",
            &record_id,
            &path,
            &language_id,
            [module.as_str(), target_hint.as_str(), path.as_str()],
        )?;
    }
    inserter.finish()?;

    Ok(())
}

pub(super) fn rebuild_call_search_documents(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    delete_search_documents_for_kind(transaction, source_scope, "call")?;
    let mut select = transaction.prepare(
        "
        SELECT call.call_id, call.path, coalesce(file.language_id, ''),
               coalesce(call.caller_name, ''), call.callee_name,
               coalesce(call.target_hint, ''), coalesce(caller.signature, ''),
               coalesce(callee.signature, '')
        FROM code_repository_calls call
        LEFT JOIN code_repository_files file
          ON file.source_scope = call.source_scope
         AND file.path = call.path
        LEFT JOIN code_repository_symbols caller
          ON caller.source_scope = call.source_scope
         AND caller.symbol_snapshot_id = call.caller_symbol_snapshot_id
        LEFT JOIN code_repository_symbols callee
          ON callee.source_scope = call.source_scope
         AND callee.symbol_snapshot_id = call.callee_symbol_snapshot_id
        WHERE call.source_scope = ?1
        ",
    )?;
    let mut rows = select.query(params![source_scope])?;
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    while let Some(row) = rows.next()? {
        let record_id = row.get::<_, String>(0)?;
        let path = row.get::<_, String>(1)?;
        let language_id = row.get::<_, String>(2)?;
        let caller_name = row.get::<_, String>(3)?;
        let callee_name = row.get::<_, String>(4)?;
        let target_hint = row.get::<_, String>(5)?;
        let caller_signature = row.get::<_, String>(6)?;
        let callee_signature = row.get::<_, String>(7)?;
        inserter.insert(
            source_scope,
            "call",
            &record_id,
            &path,
            &language_id,
            [
                caller_name.as_str(),
                callee_name.as_str(),
                target_hint.as_str(),
                caller_signature.as_str(),
                callee_signature.as_str(),
                path.as_str(),
            ],
        )?;
    }
    inserter.finish()?;

    Ok(())
}
