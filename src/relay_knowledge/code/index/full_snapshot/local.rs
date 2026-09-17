//! Direct full-snapshot callers use the same local failure identity and strict hash checks.
use super::*;
use crate::{
    code::source::{path_io::SkippedSourcePath, tree_hash_with_skipped},
    domain::{CodePathIoOperation, CodePathKind},
};

pub(super) fn build(
    registration: &CodeRepositoryRegistration,
    snapshot: ScopedSourceSnapshot,
    base_commit: Option<String>,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    #[cfg(test)]
    apply_filesystem_full_snapshot_read_mutation(&snapshot)?;
    let mut skipped = snapshot.skipped_paths;
    let mut hashes = snapshot.content_hashes;
    for _ in 0..=2 {
        std::fs::read_dir(&snapshot.root)?;
        let failures = skipped.len();
        hashes.retain(|path, _| !skipped.iter().any(|failure| failure.covers(path)));
        let tree = tree_hash_with_skipped(&hashes, &skipped);
        if let Some(pin) = &snapshot.filesystem_ref_pin {
            crate::code::source::ensure_filesystem_snapshot_matches_ref(pin, &tree)?;
        }
        let mut build = SnapshotBuild::new_with_scope_filters(
            registration,
            tree.clone(),
            tree.clone(),
            SnapshotScopeFilters {
                path_filters: snapshot.path_filters.clone(),
                language_filters: snapshot.language_filters.clone(),
            },
            true,
            hashes.len() + skipped.len(),
            0,
        );
        build.base_resolved_commit_sha = base_commit.clone();
        let readable = snapshot
            .entries
            .iter()
            .filter(|entry| hashes.contains_key(&entry.path))
            .cloned()
            .collect::<Vec<_>>();
        build.detect_and_fill_workspaces_at_commit(
            &snapshot.root,
            snapshot.kind,
            &tree,
            &readable,
            workspace_detection,
        );
        for entry in readable {
            let bytes =
                match source_snapshot_bytes(&snapshot.root, snapshot.kind, &tree, &entry.path) {
                    Ok(bytes) => bytes,
                    Err(CodeIndexError::Io(error)) => {
                        std::fs::read_dir(&snapshot.root)?;
                        skipped.push(SkippedSourcePath::from_error(
                            &entry.path,
                            CodePathKind::File,
                            CodePathIoOperation::Read,
                            error,
                        )?);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            ensure_filesystem_blobs_match_content_hashes(
                &tree,
                std::slice::from_ref(&entry.path),
                std::slice::from_ref(&bytes),
                &hashes,
            )?;
            parse_indexed_file(&mut build, &entry.path, &bytes)?;
        }
        if skipped.len() == failures {
            build.diagnostics.extend(
                skipped
                    .iter()
                    .map(|failure| failure.diagnostic(&build.repository_id, &build.source_scope)),
            );
            return Ok(build.finish());
        }
    }
    Err(CodeIndexError::InvalidInput(
        "local source keeps changing beyond the bounded snapshot replan budget".into(),
    ))
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
