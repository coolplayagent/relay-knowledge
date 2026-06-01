use std::{collections::BTreeMap, path::Path};

#[cfg(test)]
use std::{fs, path::PathBuf, sync::Mutex};

use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    CodeIndexError,
    parser::parse_indexed_file,
    scope::{ScopedSourceSnapshot, scoped_source_snapshot},
    snapshot::{SnapshotBuild, SnapshotScopeFilters},
    source::{
        ensure_filesystem_blobs_match_content_hashes, filesystem_content_hashes_for_paths,
        filesystem_tree_hash_from_path_hashes, source_snapshot_bytes,
    },
};

#[cfg(test)]
struct FullSnapshotReadMutation {
    root: PathBuf,
    path: String,
    content: Vec<u8>,
}

#[cfg(test)]
static FULL_SNAPSHOT_READ_MUTATION: Mutex<Option<FullSnapshotReadMutation>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn mutate_next_filesystem_full_snapshot_read(root: PathBuf, path: &str, content: &[u8]) {
    *FULL_SNAPSHOT_READ_MUTATION
        .lock()
        .expect("full snapshot mutation should lock") = Some(FullSnapshotReadMutation {
        root,
        path: path.to_owned(),
        content: content.to_vec(),
    });
}

pub(super) fn build_full_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let snapshot = scoped_source_snapshot(registration, selector, root, &selector.ref_selector)?;
    let filesystem_path_hashes = filesystem_full_snapshot_path_hashes(&snapshot)?;
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        snapshot.resolved_commit_sha.clone(),
        snapshot.tree_hash.clone(),
        SnapshotScopeFilters {
            path_filters: snapshot.path_filters.clone(),
            language_filters: snapshot.language_filters.clone(),
        },
        true,
        snapshot.entries.len(),
        0,
    );

    #[cfg(test)]
    apply_filesystem_full_snapshot_read_mutation(&snapshot)?;

    for entry in snapshot.entries {
        let bytes =
            source_snapshot_bytes(&snapshot.root, snapshot.kind, &build.commit, &entry.path)?;
        ensure_filesystem_blobs_match_content_hashes(
            &build.commit,
            std::slice::from_ref(&entry.path),
            std::slice::from_ref(&bytes),
            &filesystem_path_hashes,
        )?;
        parse_indexed_file(&mut build, &entry.path, &bytes)?;
    }

    Ok(build.finish())
}

fn filesystem_full_snapshot_path_hashes(
    snapshot: &ScopedSourceSnapshot,
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    if !snapshot.kind.is_filesystem() {
        return Ok(BTreeMap::new());
    }
    let paths = snapshot
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let path_hashes = filesystem_content_hashes_for_paths(&snapshot.root, &paths)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&path_hashes);
    if tree_hash != snapshot.tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {} no longer matches planned filesystem content {tree_hash}",
            snapshot.resolved_commit_sha
        )));
    }

    Ok(path_hashes)
}

#[cfg(test)]
fn apply_filesystem_full_snapshot_read_mutation(
    snapshot: &ScopedSourceSnapshot,
) -> Result<(), CodeIndexError> {
    if !snapshot.kind.is_filesystem() {
        return Ok(());
    }
    let mut mutation = FULL_SNAPSHOT_READ_MUTATION
        .lock()
        .expect("full snapshot mutation should lock");
    let Some(next) = mutation.take() else {
        return Ok(());
    };
    if next.root != snapshot.root {
        *mutation = Some(next);
        return Ok(());
    }
    fs::write(snapshot.root.join(next.path), next.content).map_err(CodeIndexError::Io)
}
