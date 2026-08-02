mod blobs;
mod filesystem_access;
mod filesystem_hashes;
mod identity;
mod language_scope;
mod snapshot;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(in crate::code) struct RegistrationSource {
    pub(in crate::code) root: PathBuf,
    pub(in crate::code) identity: String,
}

pub(in crate::code) use super::filesystem::{FileSystemScanPolicy, normalize_path_filter};
#[cfg(test)]
pub(crate) use blobs::mutate_next_filesystem_policy_read;
pub(in crate::code) use blobs::{
    source_batch_bytes_after_content_verification, source_blob_sizes_after_policy_verification,
    source_bytes_after_content_verification, source_snapshot_batch_bytes, source_snapshot_bytes,
};
#[cfg(test)]
pub(in crate::code) use filesystem_hashes::filesystem_tree_hash_for_paths;
pub(in crate::code) use filesystem_hashes::{
    ensure_filesystem_blobs_match_content_hashes, ensure_filesystem_paths_match_content_hashes,
    filesystem_content_hashes_for_paths, filesystem_tree_hash_from_path_hashes,
    source_commit_is_filesystem,
};
pub(in crate::code) use identity::{
    RepositorySourceKind, filesystem_registration_identity, registration_source, source_kind,
};
pub(in crate::code) use language_scope::source_language_filter_allows;
pub(in crate::code) use snapshot::{
    RepositorySourceSnapshot, filesystem_source_snapshot, git_tree_hash_with_submodules,
    source_snapshot,
};
