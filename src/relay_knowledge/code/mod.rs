//! Code repository indexing, parsing, identity, and source discovery boundaries.

mod common;
mod config_files;
mod error;
pub(crate) mod feature_flags;
mod identity;
mod index;
mod parser;
mod registration;
mod search;
mod source;

#[cfg(test)]
#[path = "tests/source/declarations.rs"]
mod source_declaration_tests;
#[cfg(test)]
#[path = "tests/source/filesystem.rs"]
mod source_filesystem_tests;
#[cfg(test)]
#[path = "tests/source/layout.rs"]
mod source_layout_tests;
#[cfg(test)]
#[path = "tests/source/submodule_followup.rs"]
mod source_submodule_followup_tests;
#[cfg(test)]
#[path = "tests/source/submodule_regression.rs"]
mod source_submodule_regression_tests;
#[cfg(test)]
#[path = "tests/source/submodule_review.rs"]
mod source_submodule_review_tests;
#[cfg(test)]
#[path = "tests/source/submodule.rs"]
mod source_submodule_tests;
#[cfg(test)]
#[path = "tests/fixtures.rs"]
mod test_fixtures;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "tests/source/worktree_overlay.rs"]
mod worktree_overlay_tests;

pub use error::CodeIndexError;
pub use index::{
    CodeIndexPlan, prepare_full_index_plan, prepare_full_index_plan_with_workspace_detection,
};
pub use index::{
    build_index_snapshot, changed_paths_for_diff, changed_paths_for_diff_with_filters,
    changed_paths_for_diff_with_path_filters, deleted_symbol_names_for_diff,
};
pub use registration::register_repository;
pub use scope::{partition_changed_paths_for_selector, preview_repository_scope};
pub use source::resolution::{
    resolve_repository_ref, resolve_repository_ref_with_filters,
    resolve_repository_ref_with_path_filters, resolve_repository_snapshot,
    resolve_repository_snapshot_with_filters, resolve_repository_snapshot_with_path_filters,
};

#[cfg(test)]
pub(crate) use changes::{
    reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root,
};
#[cfg(test)]
pub(crate) use git::{
    git_ls_tree_full_scan_call_count_for_root, git_show_call_count_for_root,
    reset_git_ls_tree_full_scan_call_count_for_root, reset_git_show_call_count_for_root,
};
#[cfg(test)]
pub(crate) use index::build_index_snapshot_with_base_commit;
pub(crate) use index::changed_paths_for_filesystem_diff;
#[cfg(test)]
use index::impact_paths_from_changes;
#[cfg(test)]
pub(crate) use index::mutate_next_filesystem_full_snapshot_read;
pub(crate) use index::{
    build_index_snapshot_with_workspace_detection, repository_uses_filesystem_source,
};
pub(crate) use registration::REGISTRATION_LANGUAGE_FILTER_ERROR;
pub(crate) use search::{
    SOURCE_GREP_CANDIDATE_FILE_LIMIT, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome,
    SourceGrepRequest, source_grep_matches,
};
#[cfg(test)]
use source::gitlink as source_gitlink;
pub(crate) use source::roots as source_roots;
use source::{changes, declarations as source_declarations, git, layout as scope};
pub(crate) use source_declarations::{
    SourceDeclarationMatch, safe_git_blob_path, simple_source_identifier,
    source_declarations_for_identity, source_line_defines_identity,
};

use common::{generated_detection, ids, languages};
use ids::{stable_content_hash, stable_id};
use index::snapshot;
use index::snapshot::SnapshotBuild;
#[cfg(test)]
use parser::parse_indexed_file;

#[cfg(test)]
use {
    crate::domain::{
        CodeFileFingerprint, CodeIndexMode, CodeRepositoryRegistration, CodeRepositorySelector,
    },
    changes::{tracked_entries, worktree_changed_paths},
    identity::resolve_reference_targets,
    languages::language_id,
    scope::{path_is_selected, path_scope_allows, path_scope_overlaps},
    source::{RepositorySourceKind, source_snapshot_batch_bytes},
};
