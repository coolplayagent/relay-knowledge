//! Git snapshot and tree-sitter code index construction behind blocking-worker boundaries.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

mod changes;
mod configuration;
pub(crate) mod feature_flags;
mod filesystem_delta;
mod full_snapshot;
mod git;
mod grep;
mod identity;
mod ids;
mod languages;
mod parser;
mod pipeline;
mod resolution;
mod scope;
mod snapshot;
mod source;
mod source_declarations;
mod source_gitlink;
mod source_gitlink_paths;
mod source_gitlink_selector;
mod source_paths;
pub(crate) mod source_roots;
mod worktree_overlay;

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
#[path = "tests/source/submodule_regression.rs"]
mod source_submodule_regression_tests;
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

use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeIndexSnapshot,
    CodePathTombstone, CodeRepositoryRegistration, CodeRepositorySelector,
};

#[cfg(test)]
use changes::tracked_entries;
#[cfg(test)]
use changes::worktree_changed_paths;
use changes::{
    GitChange, TrackedEntryScope, diff_changes, tracked_entries_state_with_scope,
    tracked_entries_with_scope,
};
#[cfg(test)]
pub(crate) use changes::{
    reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root,
};
use filesystem_delta::build_filesystem_delta_snapshot;
pub(crate) use filesystem_delta::changed_paths_for_filesystem_diff;
use full_snapshot::build_full_snapshot;
#[cfg(test)]
pub(crate) use full_snapshot::mutate_next_filesystem_full_snapshot_read;
#[cfg(test)]
pub(crate) use git::{
    git_ls_tree_full_scan_call_count_for_root, git_show_call_count_for_root,
    reset_git_ls_tree_full_scan_call_count_for_root, reset_git_show_call_count_for_root,
};
use git::{resolve_ref, resolve_tree};
pub(crate) use grep::{
    SOURCE_GREP_CANDIDATE_FILE_LIMIT, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome,
    SourceGrepRequest, source_grep_matches,
};
use ids::{stable_content_hash, stable_id};
use parser::parse_indexed_file;
pub use pipeline::{CodeIndexPlan, prepare_full_index_plan};
pub use resolution::{
    resolve_repository_ref, resolve_repository_ref_with_filters,
    resolve_repository_ref_with_path_filters, resolve_repository_snapshot,
    resolve_repository_snapshot_with_filters, resolve_repository_snapshot_with_path_filters,
};
use scope::{
    discover_source_layout, effective_index_path_filters, path_is_selected_with_layout,
    path_scope_overlaps, scoped_source_snapshot_for_filters,
};
pub use scope::{partition_changed_paths_for_selector, preview_repository_scope};
use snapshot::{SnapshotBuild, SnapshotScopeFilters};
#[cfg(test)]
use source::{RepositorySourceKind, source_snapshot_batch_bytes};
use source::{
    git_tree_hash_with_submodules, registration_source, source_bytes_after_content_verification,
    source_commit_is_filesystem, source_kind,
};
pub(crate) use source_declarations::{
    SourceDeclarationMatch, safe_git_blob_path, simple_source_identifier,
    source_declarations_for_identity, source_line_defines_identity,
};
use worktree_overlay::build_worktree_overlay_snapshot;

#[cfg(test)]
use {
    identity::resolve_reference_targets,
    languages::language_id,
    scope::{path_is_selected, path_scope_allows},
};

pub(crate) const REGISTRATION_LANGUAGE_FILTER_ERROR: &str = concat!(
    "registration language filters are not supported; ",
    "register the full language surface and use query-time --language filters to narrow results"
);
const MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS: usize =
    CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH;

/// Blocking code index failure.
#[derive(Debug)]
pub enum CodeIndexError {
    Io(std::io::Error),
    Git { args: Vec<String>, message: String },
    TreeSitter(String),
    InvalidInput(String),
}

impl fmt::Display for CodeIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "code index I/O failed: {error}"),
            Self::Git { args, message } => {
                write!(formatter, "git command failed ({args:?}): {message}")
            }
            Self::TreeSitter(message) => write!(formatter, "tree-sitter parse failed: {message}"),
            Self::InvalidInput(message) => write!(formatter, "invalid code index input: {message}"),
        }
    }
}

impl Error for CodeIndexError {}

impl From<std::io::Error> for CodeIndexError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Validates a code source root and creates a stable repository registration.
pub fn register_repository(
    path: impl AsRef<Path>,
    alias: impl Into<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> Result<CodeRepositoryRegistration, CodeIndexError> {
    if !language_filters.is_empty() {
        return Err(CodeIndexError::InvalidInput(
            REGISTRATION_LANGUAGE_FILTER_ERROR.to_owned(),
        ));
    }
    let source = registration_source(path.as_ref())?;
    let root_identity = source.root.display().to_string();
    let repository_id = source.identity;
    let alias = explicit_or_project_alias(alias, &source.root)?;

    CodeRepositoryRegistration::new(
        repository_id,
        alias,
        root_identity,
        path_filters,
        language_filters,
    )
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))
}

fn explicit_or_project_alias(
    alias: impl Into<String>,
    root: &Path,
) -> Result<String, CodeIndexError> {
    let alias = alias.into();
    if !alias.trim().is_empty() {
        return Ok(alias);
    }

    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            CodeIndexError::InvalidInput(
                "repository alias is empty and Git root has no project directory name".to_owned(),
            )
        })
}

/// Builds a code index snapshot from a clean Git commit or incremental diff.
pub fn build_index_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    mode: CodeIndexMode,
    previous_hashes: Vec<CodeFileFingerprint>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    build_index_snapshot_with_base_commit(registration, selector, mode, previous_hashes, None)
}

pub(crate) fn build_index_snapshot_with_base_commit(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    mode: CodeIndexMode,
    previous_hashes: Vec<CodeFileFingerprint>,
    base_resolved_commit_sha: Option<String>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let previous_hashes = previous_hashes
        .into_iter()
        .map(|fingerprint| (fingerprint.path, fingerprint.blob_hash))
        .collect::<BTreeMap<_, _>>();

    match mode {
        CodeIndexMode::Full => build_full_snapshot(registration, selector, &root),
        CodeIndexMode::Incremental { base_ref, head_ref } => build_incremental_snapshot(
            registration,
            selector,
            &root,
            &base_ref,
            &head_ref,
            &previous_hashes,
            base_resolved_commit_sha.as_deref(),
        ),
        CodeIndexMode::WorktreeOverlay => build_worktree_overlay_snapshot(
            registration,
            selector,
            &root,
            &previous_hashes,
            base_resolved_commit_sha.as_deref(),
        ),
    }
}

pub fn changed_paths_for_diff(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    changed_paths_for_diff_with_filters(root_path, base_ref, head_ref, &[], &[])
}

pub fn changed_paths_for_diff_with_path_filters(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
    path_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    changed_paths_for_diff_with_filters(root_path, base_ref, head_ref, path_filters, &[])
}

pub fn changed_paths_for_diff_with_filters(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    if source_commit_is_filesystem(base_ref) || source_commit_is_filesystem(head_ref) {
        if base_ref == head_ref {
            return Ok(Vec::new());
        }
        let base_commit = resolve_repository_ref_with_filters(
            root_path.as_ref(),
            base_ref,
            path_filters,
            language_filters,
        )?;
        let head_commit = resolve_repository_ref_with_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        if base_commit == head_commit {
            return Ok(Vec::new());
        }
        let snapshot = scoped_source_snapshot_for_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        return Ok(snapshot
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect());
    }
    if source_kind(root_path.as_ref())?.is_filesystem() {
        if base_ref == head_ref {
            return Ok(Vec::new());
        }
        let snapshot = scoped_source_snapshot_for_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        return Ok(snapshot
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect());
    }
    let changes = diff_changes(root_path.as_ref(), base_ref, head_ref)?;

    impact_paths_from_changes_with_gitlinks(
        root_path.as_ref(),
        base_ref,
        head_ref,
        changes,
        path_filters,
    )
}

#[cfg(test)]
fn impact_paths_from_changes(changes: Vec<GitChange>) -> Vec<String> {
    let mut paths = Vec::new();
    for change in changes {
        match change {
            GitChange::AddedOrModified { path }
            | GitChange::Deleted { path }
            | GitChange::TypeChanged { path } => paths.push(path),
            GitChange::Renamed { old_path, new_path } => {
                paths.push(old_path);
                paths.push(new_path);
            }
            GitChange::Copied { new_path, .. } => paths.push(new_path),
        }
    }
    paths.sort();
    paths.dedup();

    paths
}

fn impact_paths_from_changes_with_gitlinks(
    root: &Path,
    base_ref: &str,
    head_ref: &str,
    changes: Vec<GitChange>,
    path_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    let base_commit = resolve_ref(root, base_ref)?;
    let head_commit = resolve_ref(root, head_ref)?;
    let mut expander = source_gitlink::GitlinkImpactExpander::new(
        root,
        base_commit,
        head_commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    );
    let mut paths = Vec::new();
    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                push_scoped_expanded_impact_paths_or_original(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    include_base: true,
                    include_head: false,
                    path: &path,
                })?
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                push_scoped_expanded_impact_paths_or_original(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    include_base: true,
                    include_head: true,
                    path: &path,
                })?
            }
            GitChange::Renamed { old_path, new_path } => {
                push_scoped_expanded_impact_paths_or_original(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    include_base: true,
                    include_head: false,
                    path: &old_path,
                })?;
                push_scoped_expanded_impact_paths_or_original(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    include_base: false,
                    include_head: true,
                    path: &new_path,
                })?;
            }
            GitChange::Copied { new_path, .. } => {
                push_scoped_expanded_impact_paths_or_original(ImpactPathPush {
                    paths: &mut paths,
                    expander: &mut expander,
                    path_filters,
                    include_base: false,
                    include_head: true,
                    path: &new_path,
                })?;
            }
        }
    }
    paths.sort();
    paths.dedup();

    Ok(paths)
}

struct ImpactPathPush<'a, 'b> {
    paths: &'a mut Vec<String>,
    expander: &'a mut source_gitlink::GitlinkImpactExpander<'b>,
    path_filters: &'a [String],
    include_base: bool,
    include_head: bool,
    path: &'a str,
}

fn push_scoped_expanded_impact_paths_or_original(
    request: ImpactPathPush<'_, '_>,
) -> Result<(), CodeIndexError> {
    if !impact_path_scope_overlaps(request.path, request.path_filters) {
        request.paths.push(request.path.to_owned());
        return Ok(());
    }
    match request.expander.expanded_paths(
        request.path,
        request.include_base,
        request.include_head,
        &source_gitlink::GitlinkPathSelector::new(
            &|path| impact_path_scope_overlaps(path, request.path_filters),
            &|path| impact_path_scope_overlaps(path, request.path_filters),
        ),
    )? {
        Some(expanded) if !expanded.is_empty() => request.paths.extend(expanded),
        _ => request.paths.push(request.path.to_owned()),
    }

    Ok(())
}

fn impact_path_scope_overlaps(path: &str, path_filters: &[String]) -> bool {
    path_filters.is_empty()
        || path_filters
            .iter()
            .any(|filter| impact_path_overlaps_filter(path, filter))
}

fn impact_path_overlaps_filter(path: &str, filter: &str) -> bool {
    let path = normalize_impact_path_filter(path);
    let filter = normalize_impact_path_filter(filter);
    if filter == "." {
        return true;
    }
    !path.is_empty()
        && !filter.is_empty()
        && (path == filter
            || path.starts_with(&format!("{filter}/"))
            || filter.starts_with(&format!("{path}/")))
}

fn normalize_impact_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

/// Extracts symbol names removed by a diff so impact can include deleted APIs.
pub fn deleted_symbol_names_for_diff(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    if source_commit_is_filesystem(base_ref) || source_commit_is_filesystem(head_ref) {
        return Ok(Vec::new());
    }
    if source_kind(&root)?.is_filesystem() {
        return Ok(Vec::new());
    }
    let base_commit = resolve_ref(&root, base_ref)?;
    let head_commit = resolve_ref(&root, head_ref)?;
    let changes = diff_changes(&root, base_ref, head_ref)?;
    let entry_scope = tracked_entry_scope_for_selector(registration, selector);
    let base_entries = tracked_entries_with_scope(&root, &base_commit, &entry_scope)?;
    let source_layout = discover_source_layout(&base_entries);
    let context = DeletedSymbolContext {
        registration,
        selector,
        root: &root,
        base_commit: &base_commit,
        source_layout: &source_layout,
    };
    let mut names = Vec::new();

    for change in changes {
        match change {
            GitChange::Deleted { path } | GitChange::Renamed { old_path: path, .. } => {
                append_deleted_symbol_names_for_removed_path(
                    &mut names,
                    &context,
                    &base_entries,
                    &path,
                )?;
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                append_deleted_symbol_names_for_gitlink_update(
                    &mut names,
                    &context,
                    &head_commit,
                    &path,
                )?;
            }
            GitChange::Copied { .. } => {}
        }
    }
    names.sort();
    names.dedup();

    Ok(names)
}

struct DeletedSymbolContext<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
    base_commit: &'a str,
    source_layout: &'a scope::SourceLayoutDiscovery,
}

fn append_deleted_symbol_names_for_removed_path(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    base_entries: &[changes::GitTreeEntry],
    path: &str,
) -> Result<(), CodeIndexError> {
    if source_gitlink::gitlink_commit_at_tree(context.root, context.base_commit, path)?.is_some() {
        let paths = source_gitlink::bounded_expanded_paths_under(
            base_entries,
            path,
            MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        )?;
        if !paths.is_empty() {
            for path in paths {
                append_deleted_symbol_names_for_path(names, context, context.base_commit, &path)?;
            }
            return Ok(());
        }
    }

    append_deleted_symbol_names_for_path(names, context, context.base_commit, path)
}

fn append_deleted_symbol_names_for_gitlink_update(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    head_commit: &str,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_scope_overlaps(path, context.registration, context.selector) {
        return Ok(());
    }
    let Some(expansion) = source_gitlink::changed_gitlink_path_expansion(
        context.root,
        path,
        context.base_commit,
        head_commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new(
            &|path| path_scope_overlaps(path, context.registration, context.selector),
            &|path| path_scope_overlaps(path, context.registration, context.selector),
        ),
    )?
    else {
        return Ok(());
    };
    if !expansion.base_is_gitlink {
        append_deleted_symbol_names_for_path(names, context, context.base_commit, path)?;
        return Ok(());
    }
    if expansion.base_paths.is_empty() {
        return Ok(());
    }
    for path in expansion.base_paths {
        if !path_is_selected_with_layout(
            &path,
            context.registration,
            context.selector,
            context.source_layout,
        ) {
            continue;
        }
        let mut removed = symbol_names_for_path(context, context.base_commit, &path)?;
        if expansion.head_paths.contains(&path) {
            let retained = symbol_names_for_path(context, head_commit, &path)?;
            removed.retain(|name| !retained.contains(name));
        }
        names.extend(removed);
    }

    Ok(())
}

fn append_deleted_symbol_names_for_path(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    commit: &str,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_is_selected_with_layout(
        path,
        context.registration,
        context.selector,
        context.source_layout,
    ) {
        return Ok(());
    }
    names.extend(symbol_names_for_path(context, commit, path)?);

    Ok(())
}

fn symbol_names_for_path(
    context: &DeletedSymbolContext<'_>,
    commit: &str,
    path: &str,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let bytes = source_bytes_after_content_verification(context.root, commit, path, None)?;
    let mut build = SnapshotBuild::new_with_selector(
        context.registration,
        context.selector,
        commit.to_owned(),
        "deleted-symbol-seed".to_owned(),
        true,
        1,
        0,
    );
    parse_indexed_file(&mut build, path, &bytes)?;

    Ok(build
        .symbols
        .into_iter()
        .map(|symbol| symbol.name)
        .collect())
}

fn tracked_entry_scope_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> TrackedEntryScope {
    TrackedEntryScope::from_path_filters(
        registration
            .path_filters
            .iter()
            .chain(selector.path_filters.iter()),
    )
}

pub(crate) fn repository_uses_filesystem_source(
    root_path: impl AsRef<Path>,
) -> Result<bool, CodeIndexError> {
    Ok(source_kind(root_path.as_ref())?.is_filesystem())
}

fn build_incremental_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    base_ref: &str,
    head_ref: &str,
    previous_hashes: &BTreeMap<String, String>,
    base_resolved_commit_sha: Option<&str>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(base_ref)
        || source_commit_is_filesystem(head_ref)
        || base_resolved_commit_sha.is_some_and(source_commit_is_filesystem)
        || source_kind(root)?.is_filesystem()
    {
        return build_filesystem_delta_snapshot(
            registration,
            selector,
            root,
            head_ref,
            previous_hashes,
            base_resolved_commit_sha,
        );
    }
    let base_commit = resolve_ref(root, base_ref)?;
    let commit = resolve_ref(root, head_ref)?;
    let parent_tree_hash = resolve_tree(root, &commit)?;
    let changes = diff_changes(root, base_ref, head_ref)?;
    let entry_scope = tracked_entry_scope_for_selector(registration, selector);
    let base_entries = tracked_entries_with_scope(root, &base_commit, &entry_scope)?;
    let base_source_layout = discover_source_layout(&base_entries);
    let head_state = tracked_entries_state_with_scope(root, &commit, &entry_scope)?;
    let tree_hash = git_tree_hash_with_submodules(&parent_tree_hash, &head_state.submodule_states);
    let head_entries = head_state.entries;
    let source_layout = discover_source_layout(&head_entries);
    let path_filters = effective_index_path_filters(registration, selector, &source_layout);
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        commit,
        tree_hash,
        SnapshotScopeFilters {
            path_filters,
            language_filters,
        },
        false,
        changes.len(),
        0,
    );
    build.base_resolved_commit_sha = Some(base_commit.clone());
    let parse_context = ChangedPathParseContext {
        registration,
        selector,
        root,
        base_commit: &base_commit,
        previous_hashes,
        source_layout: &source_layout,
    };

    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if delete_expanded_gitlink_paths(
                    &mut build,
                    registration,
                    selector,
                    &base_entries,
                    &base_source_layout,
                    &path,
                )? {
                    continue;
                }
                if path_is_selected_with_layout(&path, registration, selector, &base_source_layout)
                {
                    build.deleted_paths.push(path);
                }
            }
            GitChange::Renamed { old_path, new_path } => {
                if delete_expanded_gitlink_paths(
                    &mut build,
                    registration,
                    selector,
                    &base_entries,
                    &base_source_layout,
                    &old_path,
                )? {
                    if !parse_expanded_gitlink_paths(
                        &mut build,
                        &parse_context,
                        &head_entries,
                        &new_path,
                    )? {
                        parse_changed_path(&mut build, &parse_context, &new_path)?;
                    }
                    continue;
                }
                if path_is_selected_with_layout(
                    &old_path,
                    registration,
                    selector,
                    &base_source_layout,
                ) {
                    build.deleted_paths.push(old_path.clone());
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        source_scope: build.source_scope.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: base_ref.to_owned(),
                        head_ref: head_ref.to_owned(),
                    });
                }
                if !parse_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &head_entries,
                    &new_path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &new_path)?;
                }
            }
            GitChange::Copied { old_path, new_path } => {
                if path_is_selected_with_layout(&new_path, registration, selector, &source_layout) {
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        source_scope: build.source_scope.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: base_ref.to_owned(),
                        head_ref: head_ref.to_owned(),
                    });
                }
                if !parse_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &head_entries,
                    &new_path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &new_path)?;
                }
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                if !parse_expanded_gitlink_change(
                    &mut build,
                    &parse_context,
                    &base_source_layout,
                    &path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &path)?;
                }
            }
        }
    }

    Ok(build.finish())
}

fn parse_expanded_gitlink_change(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    base_source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
) -> Result<bool, CodeIndexError> {
    if !path_scope_overlaps(path, context.registration, context.selector) {
        return Ok(false);
    }
    let Some(expansion) = source_gitlink::changed_gitlink_path_expansion(
        context.root,
        path,
        context.base_commit,
        &build.commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new(
            &|path| path_scope_overlaps(path, context.registration, context.selector),
            &|path| path_scope_overlaps(path, context.registration, context.selector),
        ),
    )?
    else {
        return Ok(false);
    };
    for deleted_path in expansion.base_paths.difference(&expansion.head_paths) {
        if path_is_selected_with_layout(
            deleted_path,
            context.registration,
            context.selector,
            base_source_layout,
        ) {
            build.deleted_paths.push(deleted_path.clone());
        }
    }
    if !expansion.base_is_gitlink
        && path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            base_source_layout,
        )
    {
        build.deleted_paths.push(path.to_owned());
    }
    for head_path in expansion.head_paths {
        parse_changed_path(build, context, &head_path)?;
    }

    Ok(expansion.head_is_gitlink)
}

fn parse_expanded_gitlink_paths(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    entries: &[changes::GitTreeEntry],
    path: &str,
) -> Result<bool, CodeIndexError> {
    let paths = source_gitlink::bounded_expanded_paths_under(
        entries,
        path,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    )?;
    if paths.is_empty() {
        return Ok(false);
    }
    for path in paths {
        parse_changed_path(build, context, &path)?;
    }

    Ok(true)
}

fn delete_expanded_gitlink_paths(
    build: &mut SnapshotBuild,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    entries: &[changes::GitTreeEntry],
    source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
) -> Result<bool, CodeIndexError> {
    let paths = source_gitlink::bounded_expanded_paths_under(
        entries,
        path,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    )?;
    if paths.is_empty() {
        return Ok(false);
    }
    for path in paths {
        if path_is_selected_with_layout(&path, registration, selector, source_layout) {
            build.deleted_paths.push(path);
        }
    }

    Ok(true)
}

struct ChangedPathParseContext<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
    base_commit: &'a str,
    previous_hashes: &'a BTreeMap<String, String>,
    source_layout: &'a scope::SourceLayoutDiscovery,
}

fn parse_changed_path(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_is_selected_with_layout(
        path,
        context.registration,
        context.selector,
        context.source_layout,
    ) {
        return Ok(());
    }
    let bytes = source_bytes_after_content_verification(context.root, &build.commit, path, None)?;
    let blob_hash = stable_content_hash(&bytes);
    if context.previous_hashes.get(path) == Some(&blob_hash) {
        build.skipped_unchanged_count += 1;
        return Ok(());
    }

    parse_indexed_file(build, path, &bytes)
}
