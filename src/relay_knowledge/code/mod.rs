//! Git snapshot and tree-sitter code index construction.
//!
//! This module owns blocking Git, filesystem, and parser work. Application
//! methods run these workflows behind explicit blocking-worker boundaries.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

mod changes;
mod configuration;
pub(crate) mod feature_flags;
mod git;
mod grep;
mod identity;
mod ids;
mod languages;
mod parser;
mod pipeline;
mod scope;
mod snapshot;
pub(crate) mod source_roots;

#[cfg(test)]
#[path = "tests/fixtures.rs"]
mod test_fixtures;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests/source/declarations.rs"]
mod source_declaration_tests;

#[cfg(test)]
#[path = "tests/source/layout.rs"]
mod source_layout_tests;

#[cfg(test)]
#[path = "tests/source/worktree_overlay.rs"]
mod worktree_overlay_tests;

use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexSnapshot, CodePathTombstone,
    CodeRepositoryRegistration, CodeRepositorySelector, RepositoryCodeRange,
};

use changes::{GitChange, diff_changes, tracked_entries, worktree_changed_paths};
use git::{git_bytes, git_optional, resolve_git_root, resolve_ref, resolve_tree};
pub(crate) use grep::{
    SOURCE_GREP_CANDIDATE_FILE_LIMIT, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome,
    SourceGrepRequest, source_grep_matches,
};
use ids::{stable_content_hash, stable_hash64, stable_id};
use parser::parse_indexed_file;
pub use pipeline::{CodeIndexPlan, prepare_full_index_plan};
use scope::{
    discover_source_layout, effective_index_path_filters, path_is_selected_with_layout,
    path_scope_overlaps, selection_exclusion_reason_with_layout,
};
pub use scope::{partition_changed_paths_for_selector, preview_repository_scope};
use snapshot::{SnapshotBuild, SnapshotScopeFilters};

#[cfg(test)]
use identity::resolve_reference_targets;

#[cfg(test)]
use languages::language_id;

#[cfg(test)]
use scope::{path_is_selected, path_scope_allows};

pub(crate) const REGISTRATION_LANGUAGE_FILTER_ERROR: &str = concat!(
    "registration language filters are not supported; ",
    "register the full language surface and use query-time --language filters to narrow results"
);

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

/// Validates a Git worktree and creates a stable repository registration.
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
    let root = resolve_git_root(path.as_ref())?;
    let root_identity = root.display().to_string();
    let origin = git_optional(&root, ["config", "--get", "remote.origin.url"])?
        .unwrap_or_else(|| root_identity.clone());
    let repository_id = stable_id("repo", [origin.as_str(), root_identity.as_str()]);
    let alias = explicit_or_project_alias(alias, &root)?;

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
        ),
        CodeIndexMode::WorktreeOverlay => {
            build_worktree_overlay_snapshot(registration, selector, &root, &previous_hashes)
        }
    }
}

/// Returns changed paths for impact analysis without mutating the code index.
pub fn changed_paths_for_diff(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    let changes = diff_changes(root_path.as_ref(), base_ref, head_ref)?;

    Ok(impact_paths_from_changes(changes))
}

/// Exact source declaration recovered from an indexed Git snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceDeclarationMatch {
    pub(crate) path: String,
    pub(crate) excerpt: String,
    pub(crate) byte_range: RepositoryCodeRange,
    pub(crate) line_range: RepositoryCodeRange,
}

const MAX_SOURCE_DECLARATION_FILES: usize = 8;
const MAX_SOURCE_DECLARATION_BYTES: usize = 512 * 1024;
const WORKTREE_UNTRACKED_BROAD_SEGMENTS: &[&str] = &[
    ".cache",
    ".next",
    ".nuxt",
    ".parcel-cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "target",
    "third_party",
    "vendor",
    "venv",
];

/// Reads a bounded set of indexed Git blobs and returns exact declaration lines.
pub(crate) fn source_declarations_for_identity(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    paths: Vec<String>,
    identity: &str,
) -> Result<Vec<SourceDeclarationMatch>, CodeIndexError> {
    git::validate_git_ref_arg("commit", commit)?;
    if !simple_source_identifier(identity) {
        return Ok(Vec::new());
    }

    let root = PathBuf::from(&registration.root_path);
    let mut seen = BTreeSet::new();
    let mut files_considered = 0usize;
    let mut matches = Vec::new();
    for path in paths {
        if files_considered >= MAX_SOURCE_DECLARATION_FILES {
            break;
        }
        if !safe_git_blob_path(&path) || !seen.insert(path.clone()) {
            continue;
        }
        files_considered += 1;
        let object = format!("{commit}:{path}");
        let Ok(bytes) = git::git_bytes(&root, ["show", &object]) else {
            continue;
        };
        if bytes.len() > MAX_SOURCE_DECLARATION_BYTES {
            continue;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(declaration) = first_source_declaration_match(&path, content, identity)? {
            matches.push(declaration);
        }
    }

    Ok(matches)
}

fn first_source_declaration_match(
    path: &str,
    content: &str,
    identity: &str,
) -> Result<Option<SourceDeclarationMatch>, CodeIndexError> {
    let mut byte_start = 0usize;
    for (line_index, line) in content.split_inclusive('\n').enumerate() {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let byte_end = byte_start + line_without_newline.len();
        if source_line_defines_identity(line_without_newline.trim(), identity) {
            let line_number = line_index + 1;
            return Ok(Some(SourceDeclarationMatch {
                path: path.to_owned(),
                excerpt: line_without_newline.trim().to_owned(),
                byte_range: RepositoryCodeRange::new("byte_range", byte_start, byte_end)
                    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
                line_range: RepositoryCodeRange::new("line_range", line_number, line_number)
                    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
            }));
        }
        byte_start += line.len();
    }

    Ok(None)
}

pub(crate) fn source_line_defines_identity(line: &str, identity: &str) -> bool {
    if line.is_empty() || !line_contains_identifier(line, identity) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    if line.starts_with("#define ") {
        return line
            .strip_prefix("#define ")
            .is_some_and(|suffix| line_starts_with_identifier(suffix, identity));
    }
    if line
        .strip_prefix("using ")
        .or_else(|| line.strip_prefix("typealias "))
        .is_some_and(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }
    if ["struct ", "class ", "enum ", "union ", "interface "]
        .into_iter()
        .filter_map(|prefix| line.strip_prefix(prefix))
        .any(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }

    line.contains('(') && line_looks_like_function_definition(line, identity)
}

fn line_looks_like_function_definition(line: &str, identity: &str) -> bool {
    line.match_indices(identity).any(|(identity_start, _)| {
        if !identifier_match_has_boundaries(line, identity, identity_start) {
            return false;
        }
        let prefix = line[..identity_start].trim_start();
        let suffix = line[identity_start + identity.len()..].trim_start();
        if !suffix.starts_with('(') || prefix.contains('=') {
            return false;
        }
        if prefix.chars().next_back().is_some_and(|character| {
            matches!(character, '(' | '.' | '>') || (character == ':' && !prefix.ends_with("::"))
        }) {
            return false;
        }
        !matches!(
            prefix.split_whitespace().next(),
            Some("if" | "for" | "while" | "switch" | "return")
        )
    })
}

fn line_starts_with_identifier(line: &str, identifier: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(identifier)
        && trimmed
            .get(identifier.len()..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier)
        .any(|(start, _)| identifier_match_has_boundaries(line, identifier, start))
}

fn identifier_match_has_boundaries(line: &str, identifier: &str, start: usize) -> bool {
    let end = start + identifier.len();
    line.get(..start).is_some_and(|prefix| {
        prefix
            .chars()
            .next_back()
            .is_none_or(|c| !is_identifier_char(c))
    }) && line
        .get(end..)
        .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

pub(crate) fn simple_source_identifier(value: &str) -> bool {
    !value.is_empty() && value.chars().all(is_identifier_char)
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn safe_git_blob_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && path.split('/').all(|part| !part.is_empty() && part != "..")
}

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

/// Extracts symbol names removed by a diff so impact can include deleted APIs.
pub fn deleted_symbol_names_for_diff(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let base_commit = resolve_ref(&root, base_ref)?;
    let changes = diff_changes(&root, base_ref, head_ref)?;
    let base_entries = tracked_entries(&root, &base_commit)?;
    let source_layout = discover_source_layout(&base_entries);
    let mut names = Vec::new();

    for change in changes {
        let deleted_path = match change {
            GitChange::Deleted { path } | GitChange::Renamed { old_path: path, .. } => path,
            GitChange::AddedOrModified { .. }
            | GitChange::Copied { .. }
            | GitChange::TypeChanged { .. } => continue,
        };
        if !path_is_selected_with_layout(&deleted_path, registration, selector, &source_layout) {
            continue;
        }
        let bytes = git_bytes(&root, ["show", &format!("{base_commit}:{deleted_path}")])?;
        let mut build = SnapshotBuild::new_with_selector(
            registration,
            selector,
            base_commit.clone(),
            "deleted-symbol-seed".to_owned(),
            true,
            1,
            0,
        );
        parse_indexed_file(&mut build, &deleted_path, &bytes)?;
        names.extend(build.symbols.into_iter().map(|symbol| symbol.name));
    }
    names.sort();
    names.dedup();

    Ok(names)
}

/// Resolves a repository ref selector to the exact commit used by storage.
pub fn resolve_repository_ref(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
) -> Result<String, CodeIndexError> {
    let root = resolve_git_root(root_path.as_ref())?;

    resolve_ref(&root, ref_selector)
}

/// Resolves a repository ref selector to the exact commit and tree hash.
pub fn resolve_repository_snapshot(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
) -> Result<(String, String), CodeIndexError> {
    let root = resolve_git_root(root_path.as_ref())?;
    let commit = resolve_ref(&root, ref_selector)?;
    let tree_hash = resolve_tree(&root, &commit)?;

    Ok((commit, tree_hash))
}

fn build_full_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let commit = resolve_ref(root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(root, &commit)?;
    let entries = tracked_entries(root, &commit)?;
    let source_layout = discover_source_layout(&entries);
    let path_filters = effective_index_path_filters(registration, selector, &source_layout);
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let paths = entries
        .into_iter()
        .map(|entry| entry.path)
        .filter(|path| {
            selection_exclusion_reason_with_layout(path, registration, selector, &source_layout)
                .is_none()
        })
        .collect::<Vec<_>>();
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        commit,
        tree_hash,
        SnapshotScopeFilters {
            path_filters,
            language_filters,
        },
        true,
        paths.len(),
        0,
    );

    for path in paths {
        let bytes = git_bytes(root, ["show", &format!("{}:{path}", build.commit)])?;
        parse_indexed_file(&mut build, &path, &bytes)?;
    }

    Ok(build.finish())
}

fn build_incremental_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    base_ref: &str,
    head_ref: &str,
    previous_hashes: &BTreeMap<String, String>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let base_commit = resolve_ref(root, base_ref)?;
    let commit = resolve_ref(root, head_ref)?;
    let tree_hash = resolve_tree(root, &commit)?;
    let changes = diff_changes(root, base_ref, head_ref)?;
    let base_entries = tracked_entries(root, &base_commit)?;
    let base_source_layout = discover_source_layout(&base_entries);
    let head_entries = tracked_entries(root, &commit)?;
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
        previous_hashes,
        source_layout: &source_layout,
    };

    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if path_is_selected_with_layout(&path, registration, selector, &base_source_layout)
                {
                    build.deleted_paths.push(path);
                }
            }
            GitChange::Renamed { old_path, new_path } => {
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
                parse_changed_path(&mut build, &parse_context, &new_path)?;
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
                parse_changed_path(&mut build, &parse_context, &new_path)?;
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                parse_changed_path(&mut build, &parse_context, &path)?;
            }
        }
    }

    Ok(build.finish())
}

fn build_worktree_overlay_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    previous_hashes: &BTreeMap<String, String>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let commit = resolve_ref(root, &selector.ref_selector)?;
    let head_commit = resolve_ref(root, "HEAD")?;
    if commit != head_commit {
        return Err(CodeIndexError::InvalidInput(format!(
            "worktree overlay ref '{}' resolves to {}, but checked-out HEAD is {}",
            selector.ref_selector, commit, head_commit
        )));
    }
    let status = git_bytes(
        root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let changes = worktree_changed_paths(&status);
    if changes.is_empty() {
        return build_full_snapshot(registration, selector, root);
    }
    let mut overlay_hash_input = Vec::new();
    let mut deleted_paths = Vec::new();
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    for change in &changes {
        if let Some(deleted_path) = &change.deleted_source {
            if scope::path_is_selected(deleted_path, registration, selector) {
                overlay_hash_input.extend_from_slice(b"D\0");
                overlay_hash_input.extend_from_slice(deleted_path.as_bytes());
                overlay_hash_input.push(0);
                deleted_paths.push(deleted_path.clone());
            }
        }
        let path = &change.path;
        if !path_scope_overlaps(path, registration, selector) {
            continue;
        }
        if change.is_untracked()
            && !worktree_untracked_path_is_selected(path, registration, selector)
        {
            continue;
        }
        let full_path = root.join(path);
        let metadata = match fs::symlink_metadata(&full_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if scope::path_is_selected(path, registration, selector) {
                    overlay_hash_input.extend_from_slice(b"D\0");
                    overlay_hash_input.extend_from_slice(path.as_bytes());
                    overlay_hash_input.push(0);
                    deleted_paths.push(path.clone());
                }
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            if scope::path_is_selected(path, registration, selector) {
                record_worktree_status_marker(path, &mut overlay_hash_input);
            }
            continue;
        }
        if file_type.is_dir() {
            if !change.is_untracked() || !worktree_directory_is_expandable(root, path)? {
                if scope::path_is_selected(path, registration, selector) {
                    record_worktree_status_marker(path, &mut overlay_hash_input);
                }
                continue;
            }
            for nested_path in worktree_directory_files(root, path)? {
                if worktree_untracked_path_is_selected(&nested_path, registration, selector) {
                    record_worktree_file(
                        root,
                        &nested_path,
                        previous_hashes,
                        &mut overlay_hash_input,
                        &mut files_to_parse,
                        &mut skipped_unchanged_count,
                    )?;
                }
            }
            continue;
        }
        if !file_type.is_file() {
            if scope::path_is_selected(path, registration, selector) {
                record_worktree_status_marker(path, &mut overlay_hash_input);
            }
            continue;
        }
        if scope::path_is_selected(path, registration, selector) {
            record_worktree_file(
                root,
                path,
                previous_hashes,
                &mut overlay_hash_input,
                &mut files_to_parse,
                &mut skipped_unchanged_count,
            )?;
        }
    }
    if overlay_hash_input.is_empty() {
        return build_full_snapshot(registration, selector, root);
    }

    let overlay_hash = format!("{:016x}", stable_hash64(&overlay_hash_input));
    let tree_hash = format!("worktree:{overlay_hash}");
    let overlay_commit = format!("worktree:{commit}:{overlay_hash}");
    let mut build = SnapshotBuild::new_with_selector(
        registration,
        selector,
        overlay_commit,
        tree_hash,
        false,
        changes.len(),
        skipped_unchanged_count,
    );
    build.base_resolved_commit_sha = Some(commit);
    build.deleted_paths = deleted_paths;

    for (path, bytes) in files_to_parse {
        parse_indexed_file(&mut build, &path, &bytes)?;
    }

    Ok(build.finish())
}

fn worktree_untracked_path_is_selected(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    scope::path_is_selected(path, registration, selector)
        && (!worktree_untracked_path_contains_broad_segment(path)
            || explicit_worktree_path_filter_covers(path, registration, selector))
}

fn worktree_untracked_path_contains_broad_segment(path: &str) -> bool {
    normalize_worktree_path(path)
        .split('/')
        .any(|segment| WORKTREE_UNTRACKED_BROAD_SEGMENTS.contains(&segment))
}

fn explicit_worktree_path_filter_covers(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    registration
        .path_filters
        .iter()
        .chain(selector.path_filters.iter())
        .any(|filter| explicit_worktree_filter_matches_path(path, filter))
}

fn explicit_worktree_filter_matches_path(path: &str, filter: &str) -> bool {
    let path = normalize_worktree_path(path);
    let filter = normalize_worktree_path(filter);
    if filter.is_empty() || filter == "." {
        return false;
    }

    path == filter
        || path.starts_with(&format!("{filter}/"))
        || filter.starts_with(&format!("{path}/"))
}

fn normalize_worktree_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_owned()
}

fn record_worktree_status_marker(path: &str, overlay_hash_input: &mut Vec<u8>) {
    overlay_hash_input.extend_from_slice(b"S\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
}

fn record_worktree_file(
    root: &Path,
    path: &str,
    previous_hashes: &BTreeMap<String, String>,
    overlay_hash_input: &mut Vec<u8>,
    files_to_parse: &mut Vec<(String, Vec<u8>)>,
    skipped_unchanged_count: &mut usize,
) -> Result<(), CodeIndexError> {
    let bytes = fs::read(root.join(path))?;
    let blob_hash = stable_content_hash(&bytes);
    overlay_hash_input.extend_from_slice(b"F\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
    overlay_hash_input.extend_from_slice(blob_hash.as_bytes());
    overlay_hash_input.push(0);
    if previous_hashes.get(path) == Some(&blob_hash) {
        *skipped_unchanged_count += 1;
        return Ok(());
    }
    files_to_parse.push((path.to_owned(), bytes));

    Ok(())
}

fn worktree_directory_files(
    root: &Path,
    relative_dir: &str,
) -> Result<Vec<String>, CodeIndexError> {
    if !worktree_directory_is_expandable(root, relative_dir)? {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_worktree_directory_files(root, Path::new(relative_dir), &mut files)?;
    files.sort();

    Ok(files)
}

fn worktree_directory_is_expandable(
    root: &Path,
    relative_dir: &str,
) -> Result<bool, CodeIndexError> {
    let full_path = root.join(relative_dir);
    let metadata = fs::symlink_metadata(&full_path)?;
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }

    Ok(!contains_git_metadata(root, Path::new(relative_dir))?)
}

fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn collect_worktree_directory_files(
    root: &Path,
    relative: &Path,
    files: &mut Vec<String>,
) -> Result<(), CodeIndexError> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if entry.file_name() == ".git" || contains_git_metadata(root, &path)? {
                continue;
            }
            collect_worktree_directory_files(root, &path, files)?;
        } else if file_type.is_file() {
            files.push(path.to_string_lossy().replace('\\', "/"));
        }
    }

    Ok(())
}

struct ChangedPathParseContext<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
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
    let object = format!("{}:{path}", build.commit);
    let bytes = git_bytes(context.root, ["show", &object])?;
    let blob_hash = stable_content_hash(&bytes);
    if context.previous_hashes.get(path) == Some(&blob_hash) {
        build.skipped_unchanged_count += 1;
        return Ok(());
    }

    parse_indexed_file(build, path, &bytes)
}
