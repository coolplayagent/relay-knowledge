use std::{collections::BTreeMap, fs, path::Path};

use super::{CodeIndexError, stable_content_hash};

pub(super) struct WorktreeFileOutputs<'a> {
    pub(super) overlay_hash_input: &'a mut Vec<u8>,
    pub(super) deleted_paths: &'a mut Vec<String>,
    pub(super) files_to_parse: &'a mut Vec<(String, Vec<u8>)>,
    pub(super) skipped_unchanged_count: &'a mut usize,
}

pub(super) fn record_status_marker(path: &str, overlay_hash_input: &mut Vec<u8>) {
    overlay_hash_input.extend_from_slice(b"S\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
}

pub(super) fn record_deleted_path(
    path: &str,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) {
    overlay_hash_input.extend_from_slice(b"D\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
    deleted_paths.push(path.to_owned());
}

pub(super) fn record_unparseable_path(
    path: &str,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) {
    record_status_marker(path, overlay_hash_input);
    record_deleted_path(path, overlay_hash_input, deleted_paths);
}

pub(super) fn record_file_as(
    root: &Path,
    source_path: &str,
    indexed_path: &str,
    previous_hashes: &BTreeMap<String, String>,
    outputs: &mut WorktreeFileOutputs<'_>,
) -> Result<(), CodeIndexError> {
    let bytes = fs::read(root.join(source_path))?;
    let blob_hash = stable_content_hash(&bytes);
    outputs.overlay_hash_input.extend_from_slice(b"F\0");
    outputs
        .overlay_hash_input
        .extend_from_slice(indexed_path.as_bytes());
    outputs.overlay_hash_input.push(0);
    outputs
        .overlay_hash_input
        .extend_from_slice(blob_hash.as_bytes());
    outputs.overlay_hash_input.push(0);
    let was_deleted = outputs
        .deleted_paths
        .iter()
        .any(|path| path == indexed_path);
    outputs.deleted_paths.retain(|path| path != indexed_path);
    if previous_hashes.get(indexed_path) == Some(&blob_hash) && !was_deleted {
        *outputs.skipped_unchanged_count += 1;
        return Ok(());
    }
    outputs
        .files_to_parse
        .retain(|(path, _)| path != indexed_path);
    outputs
        .files_to_parse
        .push((indexed_path.to_owned(), bytes));

    Ok(())
}

#[cfg(test)]
#[path = "recording_tests.rs"]
mod tests;
