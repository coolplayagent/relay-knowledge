//! Deterministically partitions an in-memory incremental snapshot into file-owned fact batches.

use std::{collections::BTreeMap, ops::Range};

use crate::{
    domain::{CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSnapshot},
    storage::StorageError,
};

pub(super) struct DeltaBatchPlan<'a> {
    snapshot: &'a CodeIndexSnapshot,
    ranges: Vec<Range<usize>>,
}

impl<'a> DeltaBatchPlan<'a> {
    pub(super) fn new(
        snapshot: &'a CodeIndexSnapshot,
        budget: CodeIndexResourceBudget,
    ) -> Result<Self, StorageError> {
        let row_counts = file_row_counts(snapshot)?;
        let mut ranges = Vec::new();
        let mut start = 0usize;
        let mut files = 0usize;
        let mut bytes = 0usize;
        let mut rows = 0usize;
        for (index, file) in snapshot.files.iter().enumerate() {
            let file_rows = *row_counts.get(file.path.as_str()).ok_or_else(|| {
                StorageError::Invariant(format!(
                    "durable delta file '{}' lost its row-count owner",
                    file.path
                ))
            })?;
            if file.byte_len > budget.max_bytes_per_batch || file_rows > budget.max_rows_per_batch {
                return Err(StorageError::CapacityExceeded(format!(
                    "durable delta file '{}' cannot fit one frozen writer quantum for scope '{}'",
                    file.path, snapshot.source_scope
                )));
            }
            let next_files = files.checked_add(1).ok_or_else(|| capacity(snapshot))?;
            let next_bytes = bytes
                .checked_add(file.byte_len)
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
                bytes = file.byte_len;
                rows = file_rows;
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

fn file_row_counts(snapshot: &CodeIndexSnapshot) -> Result<BTreeMap<&str, usize>, StorageError> {
    let mut counts = BTreeMap::new();
    for file in &snapshot.files {
        if counts.insert(file.path.as_str(), 1usize).is_some() {
            return Err(StorageError::Invariant(format!(
                "durable delta contains duplicate file path '{}'",
                file.path
            )));
        }
    }
    add_paths(
        &mut counts,
        snapshot.symbols.iter().map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .references
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot.imports.iter().map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .dependencies
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .feature_flags
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .framework_nodes
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .framework_edges
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot.routes.iter().map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot.chunks.iter().map(|record| record.path.as_str()),
        snapshot,
    )?;
    add_paths(
        &mut counts,
        snapshot
            .diagnostics
            .iter()
            .map(|record| record.path.as_str()),
        snapshot,
    )?;
    // Calls are regenerated from call-shaped references during finalization,
    // but their eventual rows still consume the owning file's batch budget.
    add_paths(
        &mut counts,
        snapshot.calls.iter().map(|record| record.path.as_str()),
        snapshot,
    )?;
    Ok(counts)
}

fn add_paths<'a>(
    counts: &mut BTreeMap<&'a str, usize>,
    paths: impl IntoIterator<Item = &'a str>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    for path in paths {
        let count = counts.get_mut(path).ok_or_else(|| orphan(path, snapshot))?;
        *count = count.checked_add(1).ok_or_else(|| capacity(snapshot))?;
    }
    Ok(())
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
