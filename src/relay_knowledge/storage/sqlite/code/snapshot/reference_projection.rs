//! Owns bounded direct grouped reference-search projection writes.

use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use crate::{
    domain::{CodeIndexResourceBudget, CodeIndexSnapshot},
    storage::StorageError,
};

use super::super::search::{ReferenceSearchGroupStorage, insert_search_document};

const REFERENCE_KIND: &str = "reference";

struct DirectReferenceSearchGroup {
    group_id: String,
    name: String,
    kind: String,
    path: String,
    target_hint: String,
    language_id: String,
    occurrence_count: usize,
}

type DirectReferenceSearchIdentity = (String, String, String, String);
type DirectReferenceSearchGroups =
    BTreeMap<DirectReferenceSearchIdentity, DirectReferenceSearchGroup>;

pub(super) fn insert_direct_reference_search_projection(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    file_languages_by_path: &BTreeMap<&str, &str>,
    resource_budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let groups = direct_reference_search_groups(snapshot, file_languages_by_path)?;
    let document_limit = resource_budget.max_rows_per_batch.saturating_sub(2) / 3;
    if resource_budget.max_rows_per_batch < 2
        || resource_budget.max_bytes_per_batch == 0
        || groups.len() > document_limit
    {
        return Err(capacity_error(&snapshot.source_scope));
    }
    let mut projection_bytes = if snapshot.full_replace {
        checked_mul(
            snapshot.source_scope.len().saturating_add(64),
            2,
            "manifest bytes",
        )?
    } else {
        snapshot.source_scope.len().saturating_add(64)
    };
    if projection_bytes > resource_budget.max_bytes_per_batch {
        return Err(capacity_error(&snapshot.source_scope));
    }
    for group in groups.values() {
        projection_bytes = checked_add(
            projection_bytes,
            group_persisted_bytes(&snapshot.source_scope, group)?,
            "direct projection bytes",
        )?;
        if projection_bytes > resource_budget.max_bytes_per_batch {
            return Err(capacity_error(&snapshot.source_scope));
        }
    }
    for group in groups.values() {
        transaction.execute(
            "INSERT INTO code_repository_reference_search_groups (
                 source_scope, group_id, name, kind, path, target_hint,
                 language_id, occurrence_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot.source_scope,
                group.group_id,
                group.name,
                group.kind,
                group.path,
                group.target_hint,
                group.language_id,
                group.occurrence_count,
            ],
        )?;
        insert_search_document(
            transaction,
            &snapshot.source_scope,
            REFERENCE_KIND,
            &group.group_id,
            &group.path,
            &group.language_id,
            [
                group.name.as_str(),
                group.kind.as_str(),
                group.target_hint.as_str(),
                group.path.as_str(),
            ],
        )?;
    }
    if snapshot.full_replace {
        transaction.execute(
            "INSERT INTO code_repository_reference_search_manifests (
                 source_scope, projection_version, reference_count, group_count
             ) VALUES (?1, 2, 0, 0)",
            params![snapshot.source_scope],
        )?;
    }
    let changed = transaction.execute(
        "UPDATE code_repository_reference_search_manifests
         SET projection_version = 2,
             reference_count = reference_count + ?2,
             group_count = group_count + ?3
         WHERE source_scope = ?1",
        params![
            snapshot.source_scope,
            snapshot.references.len(),
            groups.len()
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "incremental scope '{}' is missing its cloned grouped reference-search manifest",
            snapshot.source_scope
        )));
    }
    Ok(())
}

pub(super) fn require_full_grouped_projection_within_budget(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    resource_budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    require_target_reference_owner_empty(transaction, &snapshot.source_scope)?;
    let manifest_bytes = checked_mul(
        snapshot.source_scope.len().saturating_add(64),
        2,
        "manifest bytes",
    )?;
    let languages = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let groups = direct_reference_search_groups(snapshot, &languages)?;
    let bytes = groups.values().try_fold(manifest_bytes, |bytes, group| {
        checked_add(
            bytes,
            group_persisted_bytes(&snapshot.source_scope, group)?,
            "full projection bytes",
        )
    })?;
    let rows = checked_add(
        checked_mul(groups.len(), 3, "full projection rows")?,
        2,
        "full manifest rows",
    )?;
    if rows > resource_budget.max_rows_per_batch || bytes > resource_budget.max_bytes_per_batch {
        return Err(capacity_error(&snapshot.source_scope));
    }
    Ok(())
}

fn require_target_reference_owner_empty(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let existing_owner = transaction.query_row(
        "SELECT
             EXISTS (SELECT 1 FROM code_repository_reference_search_groups
                     WHERE source_scope = ?1 LIMIT 1)
             OR EXISTS (SELECT 1 FROM code_repository_reference_search_manifests
                       WHERE source_scope = ?1 LIMIT 1)
             OR EXISTS (SELECT 1 FROM code_repository_search_metadata
                       WHERE source_scope = ?1 AND document_kind = 'reference' LIMIT 1)
             ",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if existing_owner {
        return Err(StorageError::CapacityExceeded(format!(
            "direct reference-search replacement for scope '{source_scope}' requires durable staged cleanup"
        )));
    }
    Ok(())
}

pub(super) fn reference_search_group_count(
    snapshot: &CodeIndexSnapshot,
) -> Result<usize, StorageError> {
    let languages = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.language_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    Ok(direct_reference_search_groups(snapshot, &languages)?.len())
}

fn direct_reference_search_groups(
    snapshot: &CodeIndexSnapshot,
    file_languages_by_path: &BTreeMap<&str, &str>,
) -> Result<DirectReferenceSearchGroups, StorageError> {
    let mut groups = BTreeMap::new();
    for reference in &snapshot.references {
        let target_hint = reference.target_hint.as_deref().unwrap_or_default();
        let language_id = file_languages_by_path
            .get(reference.path.as_str())
            .copied()
            .unwrap_or_default();
        let key = (
            reference.name.clone(),
            reference.kind.clone(),
            reference.path.clone(),
            target_hint.to_owned(),
        );
        let group = groups
            .entry(key)
            .or_insert_with(|| DirectReferenceSearchGroup {
                group_id: reference.reference_id.clone(),
                name: reference.name.clone(),
                kind: reference.kind.clone(),
                path: reference.path.clone(),
                target_hint: target_hint.to_owned(),
                language_id: language_id.to_owned(),
                occurrence_count: 0,
            });
        if reference.reference_id < group.group_id {
            group.group_id.clone_from(&reference.reference_id);
        }
        group.occurrence_count = checked_add(group.occurrence_count, 1, "occurrence count")?;
    }
    Ok(groups)
}

fn group_persisted_bytes(
    source_scope: &str,
    group: &DirectReferenceSearchGroup,
) -> Result<usize, StorageError> {
    ReferenceSearchGroupStorage {
        source_scope,
        group_id: &group.group_id,
        name: &group.name,
        reference_kind: &group.kind,
        path: &group.path,
        target_hint: &group.target_hint,
        language_id: &group.language_id,
    }
    .persisted_byte_upper_bound()
    .ok_or_else(|| {
        StorageError::CapacityExceeded(
            "direct grouped reference-search persisted group bytes exceed platform capacity"
                .to_owned(),
        )
    })
}

fn checked_add(left: usize, right: usize, label: &str) -> Result<usize, StorageError> {
    left.checked_add(right).ok_or_else(|| {
        StorageError::CapacityExceeded(format!(
            "direct grouped reference-search {label} exceeds platform capacity"
        ))
    })
}

fn checked_mul(left: usize, right: usize, label: &str) -> Result<usize, StorageError> {
    left.checked_mul(right).ok_or_else(|| {
        StorageError::CapacityExceeded(format!(
            "direct grouped reference-search {label} exceeds platform capacity"
        ))
    })
}

fn capacity_error(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "direct grouped reference-search projection for scope '{source_scope}' exceeds one authoritative writer quantum; use durable full indexing"
    ))
}

#[cfg(test)]
#[path = "reference_projection_tests.rs"]
mod tests;
