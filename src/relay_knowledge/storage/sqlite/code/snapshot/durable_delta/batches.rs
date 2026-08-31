//! Deterministically partitions an in-memory incremental snapshot into file-owned fact batches.

use std::{collections::BTreeMap, ops::Range};

use serde::Serialize;

use crate::{
    domain::{CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSnapshot},
    storage::StorageError,
};

// Each searchable fact writes one FTS row and one exact-owner metadata row.
const SEARCH_DOCUMENT_ROW_COUNT: usize = 2;
// Reference finalization can own one grouped fact, FTS row, and metadata row
// for each reference; charging that upper bound keeps grouping conservative.
const REFERENCE_SEARCH_ROW_COUNT: usize = 3;
// The staging manifest is inserted in the same writer quantum as the facts.
const DURABLE_BATCH_CONTROL_ROW_COUNT: usize = 1;

pub(super) struct DeltaBatchPlan<'a> {
    snapshot: &'a CodeIndexSnapshot,
    ranges: Vec<Range<usize>>,
}

impl<'a> DeltaBatchPlan<'a> {
    pub(super) fn new(
        snapshot: &'a CodeIndexSnapshot,
        budget: CodeIndexResourceBudget,
    ) -> Result<Self, StorageError> {
        let surfaces = file_surfaces(snapshot)?;
        let control_bytes = batch_control_bytes(snapshot)?;
        let mut ranges = Vec::new();
        let mut start = 0usize;
        let mut files = 0usize;
        let mut bytes = control_bytes;
        let mut rows = DURABLE_BATCH_CONTROL_ROW_COUNT;
        for (index, file) in snapshot.files.iter().enumerate() {
            let surface = surfaces.get(file.path.as_str()).ok_or_else(|| {
                StorageError::Invariant(format!(
                    "durable delta file '{}' lost its owned-fact surface",
                    file.path
                ))
            })?;
            let file_bytes = surface.bytes;
            let file_rows = surface.rows;
            if file_bytes
                .checked_add(control_bytes)
                .ok_or_else(|| capacity(snapshot))?
                > budget.max_bytes_per_batch
                || file_rows
                    .checked_add(DURABLE_BATCH_CONTROL_ROW_COUNT)
                    .ok_or_else(|| capacity(snapshot))?
                    > budget.max_rows_per_batch
            {
                return Err(StorageError::CapacityExceeded(format!(
                    "durable delta file '{}' owned fact surface cannot fit one frozen writer quantum for scope '{}'",
                    file.path, snapshot.source_scope
                )));
            }
            let next_files = files.checked_add(1).ok_or_else(|| capacity(snapshot))?;
            let next_bytes = bytes
                .checked_add(file_bytes)
                .ok_or_else(|| capacity(snapshot))?;
            let next_rows = rows
                .checked_add(file_rows)
                .ok_or_else(|| capacity(snapshot))?;
            if files > 0
                && (next_files > budget.max_files_per_batch
                    || next_bytes > budget.max_bytes_per_batch
                    || next_rows > budget.max_rows_per_batch)
            {
                ranges.push(start..index);
                start = index;
                files = 1;
                bytes = control_bytes
                    .checked_add(file_bytes)
                    .ok_or_else(|| capacity(snapshot))?;
                rows = DURABLE_BATCH_CONTROL_ROW_COUNT
                    .checked_add(file_rows)
                    .ok_or_else(|| capacity(snapshot))?;
            } else {
                files = next_files;
                bytes = next_bytes;
                rows = next_rows;
            }
        }
        if start < snapshot.files.len() {
            ranges.push(start..snapshot.files.len());
        }
        Ok(Self { snapshot, ranges })
    }

    pub(super) fn len(&self) -> usize {
        self.ranges.len()
    }

    pub(super) fn batch(
        &self,
        ordinal: usize,
        batch_index: usize,
    ) -> Result<CodeIndexBatch, StorageError> {
        let range = self.ranges.get(ordinal).cloned().ok_or_else(|| {
            StorageError::Invariant(format!(
                "durable delta batch ordinal {ordinal} exceeds its {}-batch plan",
                self.ranges.len()
            ))
        })?;
        let files = self.snapshot.files[range].to_vec();
        let selected = files
            .iter()
            .map(|file| file.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let owns = |path: &str| selected.contains(path);
        let parsed_byte_count = files.iter().try_fold(0usize, |total, file| {
            total
                .checked_add(file.byte_len)
                .ok_or_else(|| capacity(self.snapshot))
        })?;
        Ok(CodeIndexBatch {
            repository_id: self.snapshot.repository_id.clone(),
            source_scope: self.snapshot.source_scope.clone(),
            batch_index,
            parsed_byte_count,
            files,
            symbols: self
                .snapshot
                .symbols
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            references: self
                .snapshot
                .references
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            imports: self
                .snapshot
                .imports
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            dependencies: self
                .snapshot
                .dependencies
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            feature_flags: self
                .snapshot
                .feature_flags
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            framework_nodes: self
                .snapshot
                .framework_nodes
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            framework_edges: self
                .snapshot
                .framework_edges
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            routes: self
                .snapshot
                .routes
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            chunks: self
                .snapshot
                .chunks
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
            diagnostics: self
                .snapshot
                .diagnostics
                .iter()
                .filter(|record| owns(&record.path))
                .cloned()
                .collect(),
        })
    }
}

#[derive(Default)]
struct FileSurface {
    rows: usize,
    bytes: usize,
}

fn file_surfaces(
    snapshot: &CodeIndexSnapshot,
) -> Result<BTreeMap<&str, FileSurface>, StorageError> {
    let mut surfaces = BTreeMap::new();
    for file in &snapshot.files {
        let serialized = serialized_bytes(file, snapshot)?;
        let bytes = persisted_bytes(serialized, 0, snapshot)?
            .checked_add(file.byte_len)
            .ok_or_else(|| capacity(snapshot))?;
        if surfaces
            .insert(file.path.as_str(), FileSurface { rows: 1, bytes })
            .is_some()
        {
            return Err(StorageError::Invariant(format!(
                "durable delta contains duplicate file path '{}'",
                file.path
            )));
        }
    }

    add_records(
        &mut surfaces,
        snapshot
            .symbols
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .references
            .iter()
            .map(|record| (record.path.as_str(), record)),
        REFERENCE_SEARCH_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .imports
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .dependencies
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .feature_flags
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .framework_nodes
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .framework_edges
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .routes
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .chunks
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    add_records(
        &mut surfaces,
        snapshot
            .diagnostics
            .iter()
            .map(|record| (record.path.as_str(), record)),
        0,
        snapshot,
    )?;
    // Calls are regenerated from call-shaped references during finalization,
    // but their eventual rows and search projection still consume the owning
    // file's frozen writer budget.
    add_records(
        &mut surfaces,
        snapshot
            .calls
            .iter()
            .map(|record| (record.path.as_str(), record)),
        SEARCH_DOCUMENT_ROW_COUNT,
        snapshot,
    )?;
    Ok(surfaces)
}

fn add_records<'path, 'record, T: Serialize + 'record>(
    surfaces: &mut BTreeMap<&'path str, FileSurface>,
    records: impl IntoIterator<Item = (&'path str, &'record T)>,
    derived_row_count: usize,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    for (path, record) in records {
        let surface = surfaces
            .get_mut(path)
            .ok_or_else(|| orphan(path, snapshot))?;
        let serialized = serialized_bytes(record, snapshot)?;
        surface.rows = surface
            .rows
            .checked_add(1)
            .and_then(|rows| rows.checked_add(derived_row_count))
            .ok_or_else(|| capacity(snapshot))?;
        surface.bytes = surface
            .bytes
            .checked_add(persisted_bytes(serialized, derived_row_count, snapshot)?)
            .ok_or_else(|| capacity(snapshot))?;
    }
    Ok(())
}

fn serialized_bytes<T: Serialize>(
    value: &T,
    snapshot: &CodeIndexSnapshot,
) -> Result<usize, StorageError> {
    serde_json::to_vec(value)
        .map(|encoded| encoded.len())
        .map_err(|error| {
            StorageError::Invariant(format!(
                "durable delta surface for scope '{}' could not be serialized: {error}",
                snapshot.source_scope
            ))
        })
}

fn persisted_bytes(
    serialized: usize,
    derived_row_count: usize,
    snapshot: &CodeIndexSnapshot,
) -> Result<usize, StorageError> {
    let fact_bytes = serialized
        .checked_add(super::super::admission::ROW_STORAGE_OVERHEAD_BYTES)
        .ok_or_else(|| capacity(snapshot))?;
    let derived_bytes = serialized
        .checked_mul(super::super::admission::SNAPSHOT_SEARCH_EXPANSION)
        .and_then(|bytes| bytes.checked_add(super::super::admission::ROW_STORAGE_OVERHEAD_BYTES))
        .ok_or_else(|| capacity(snapshot))?;
    fact_bytes
        .checked_add(
            derived_bytes
                .checked_mul(derived_row_count)
                .ok_or_else(|| capacity(snapshot))?,
        )
        .ok_or_else(|| capacity(snapshot))
}

fn batch_control_bytes(snapshot: &CodeIndexSnapshot) -> Result<usize, StorageError> {
    snapshot
        .source_scope
        .len()
        .checked_add(super::super::admission::ROW_STORAGE_OVERHEAD_BYTES)
        .ok_or_else(|| capacity(snapshot))
}

fn orphan(path: &str, snapshot: &CodeIndexSnapshot) -> StorageError {
    StorageError::Invariant(format!(
        "durable delta fact path '{path}' has no file owner in scope '{}'",
        snapshot.source_scope
    ))
}

fn capacity(snapshot: &CodeIndexSnapshot) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "durable delta batch plan for scope '{}' exceeds platform capacity",
        snapshot.source_scope
    ))
}

#[cfg(test)]
#[path = "batches_tests.rs"]
mod tests;
