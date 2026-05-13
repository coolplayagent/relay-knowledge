//! Git snapshot and tree-sitter code index construction.
//!
//! This module owns blocking Git, filesystem, and parser work. Application
//! methods run these workflows behind explicit blocking-worker boundaries.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

mod languages;
mod parser;
mod scope;

#[cfg(test)]
mod tests;

use crate::domain::{
    CodeCallRecord, CodeFileFingerprint, CodeIndexMode, CodeIndexSnapshot, CodePathTombstone,
    CodeRepositoryRegistration, CodeRepositorySelector, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

use parser::parse_indexed_file;
use scope::{
    load_ignore_rules, load_ignore_rules_from_commit, path_is_selected_with_rules,
    path_scope_overlaps, selection_exclusion_reason,
};
pub use scope::{partition_changed_paths_for_selector, preview_repository_scope};

#[cfg(test)]
use languages::language_id;

#[cfg(test)]
use scope::{path_is_selected, path_scope_allows};

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
    let root = resolve_git_root(path.as_ref())?;
    let root_identity = root.display().to_string();
    let origin = git_optional(&root, ["config", "--get", "remote.origin.url"])?
        .unwrap_or_else(|| root_identity.clone());
    let repository_id = stable_id("repo", [origin.as_str(), root_identity.as_str()]);

    CodeRepositoryRegistration::new(
        repository_id,
        alias,
        root_identity,
        path_filters,
        language_filters,
    )
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))
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
    let head_commit = resolve_ref(&root, head_ref)?;
    let changes = diff_changes(&root, base_ref, head_ref)?;
    let ignore_rules = load_ignore_rules_from_commit(&root, &head_commit)?;
    let mut names = Vec::new();

    for change in changes {
        let deleted_path = match change {
            GitChange::Deleted { path } | GitChange::Renamed { old_path: path, .. } => path,
            GitChange::AddedOrModified { .. }
            | GitChange::Copied { .. }
            | GitChange::TypeChanged { .. } => continue,
        };
        if !path_is_selected_with_rules(&deleted_path, registration, selector, &ignore_rules) {
            continue;
        }
        let bytes = git_bytes(&root, ["show", &format!("{base_commit}:{deleted_path}")])?;
        let mut build = SnapshotBuild::new(
            registration,
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

fn build_full_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let commit = resolve_ref(root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(root, &commit)?;
    let ignore_rules = load_ignore_rules_from_commit(root, &commit)?;
    let paths = tracked_paths(root, &commit)?
        .into_iter()
        .filter(|path| {
            selection_exclusion_reason(path, registration, selector, &ignore_rules).is_none()
        })
        .collect::<Vec<_>>();
    let mut build = SnapshotBuild::new(registration, commit, tree_hash, true, paths.len(), 0);

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
    let base_ignore_rules = load_ignore_rules_from_commit(root, &base_commit)?;
    let ignore_rules = load_ignore_rules_from_commit(root, &commit)?;
    let mut build = SnapshotBuild::new(registration, commit, tree_hash, false, changes.len(), 0);

    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if path_is_selected_with_rules(&path, registration, selector, &base_ignore_rules) {
                    build.deleted_paths.push(path);
                }
            }
            GitChange::Renamed { old_path, new_path } => {
                if path_is_selected_with_rules(
                    &old_path,
                    registration,
                    selector,
                    &base_ignore_rules,
                ) {
                    build.deleted_paths.push(old_path.clone());
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: base_ref.to_owned(),
                        head_ref: head_ref.to_owned(),
                    });
                }
                parse_changed_path(
                    &mut build,
                    registration,
                    selector,
                    root,
                    &new_path,
                    previous_hashes,
                    &ignore_rules,
                )?;
            }
            GitChange::Copied { old_path, new_path } => {
                if path_is_selected_with_rules(&new_path, registration, selector, &ignore_rules) {
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: base_ref.to_owned(),
                        head_ref: head_ref.to_owned(),
                    });
                }
                parse_changed_path(
                    &mut build,
                    registration,
                    selector,
                    root,
                    &new_path,
                    previous_hashes,
                    &ignore_rules,
                )?;
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                parse_changed_path(
                    &mut build,
                    registration,
                    selector,
                    root,
                    &path,
                    previous_hashes,
                    &ignore_rules,
                )?;
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
    let ignore_rules = load_ignore_rules(root)?;

    for change in &changes {
        if let Some(deleted_path) = &change.deleted_source {
            if path_is_selected_with_rules(deleted_path, registration, selector, &ignore_rules) {
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
        let full_path = root.join(path);
        let metadata = match fs::symlink_metadata(&full_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if path_is_selected_with_rules(path, registration, selector, &ignore_rules) {
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
            if path_is_selected_with_rules(path, registration, selector, &ignore_rules) {
                record_worktree_status_marker(path, &mut overlay_hash_input);
            }
            continue;
        }
        if file_type.is_dir() {
            if !change.is_untracked() || !worktree_directory_is_expandable(root, path)? {
                if path_is_selected_with_rules(path, registration, selector, &ignore_rules) {
                    record_worktree_status_marker(path, &mut overlay_hash_input);
                }
                continue;
            }
            for nested_path in worktree_directory_files(root, path)? {
                if path_is_selected_with_rules(&nested_path, registration, selector, &ignore_rules)
                {
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
            if path_is_selected_with_rules(path, registration, selector, &ignore_rules) {
                record_worktree_status_marker(path, &mut overlay_hash_input);
            }
            continue;
        }
        if path_is_selected_with_rules(path, registration, selector, &ignore_rules) {
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
    let mut build = SnapshotBuild::new(
        registration,
        overlay_commit,
        tree_hash,
        false,
        changes.len(),
        skipped_unchanged_count,
    );
    build.deleted_paths = deleted_paths;

    for (path, bytes) in files_to_parse {
        parse_indexed_file(&mut build, &path, &bytes)?;
    }

    Ok(build.finish())
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

fn parse_changed_path(
    build: &mut SnapshotBuild,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    path: &str,
    previous_hashes: &BTreeMap<String, String>,
    ignore_rules: &[scope::IgnoreRule],
) -> Result<(), CodeIndexError> {
    if !path_is_selected_with_rules(path, registration, selector, ignore_rules) {
        return Ok(());
    }
    let object = format!("{}:{path}", build.commit);
    let bytes = git_bytes(root, ["show", &object])?;
    let blob_hash = stable_content_hash(&bytes);
    if previous_hashes.get(path) == Some(&blob_hash) {
        build.skipped_unchanged_count += 1;
        return Ok(());
    }

    parse_indexed_file(build, path, &bytes)
}

pub(super) struct SnapshotBuild {
    pub(super) repository_id: String,
    commit: String,
    tree_hash: String,
    full_replace: bool,
    changed_path_count: usize,
    pub(super) skipped_unchanged_count: usize,
    pub(super) deleted_paths: Vec<String>,
    tombstones: Vec<CodePathTombstone>,
    pub(super) files: Vec<crate::domain::RepositoryCodeFileRecord>,
    pub(super) symbols: Vec<RepositoryCodeSymbolRecord>,
    pub(super) references: Vec<RepositoryCodeReferenceRecord>,
    pub(super) imports: Vec<crate::domain::CodeImportRecord>,
    calls: Vec<CodeCallRecord>,
    pub(super) chunks: Vec<crate::domain::RepositoryCodeChunkRecord>,
    pub(super) diagnostics: Vec<crate::domain::CodeFileDiagnostic>,
}

impl SnapshotBuild {
    fn new(
        registration: &CodeRepositoryRegistration,
        commit: String,
        tree_hash: String,
        full_replace: bool,
        changed_path_count: usize,
        skipped_unchanged_count: usize,
    ) -> Self {
        Self {
            repository_id: registration.repository_id.clone(),
            commit,
            tree_hash,
            full_replace,
            changed_path_count,
            skipped_unchanged_count,
            deleted_paths: Vec::new(),
            tombstones: Vec::new(),
            files: Vec::new(),
            symbols: Vec::new(),
            references: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(mut self) -> CodeIndexSnapshot {
        resolve_reference_targets(&self.symbols, &mut self.references);
        self.calls = self
            .references
            .iter()
            .filter(|reference| reference.kind == "call")
            .map(|reference| CodeCallRecord {
                repository_id: reference.repository_id.clone(),
                call_id: stable_id(
                    "call",
                    [
                        self.repository_id.as_str(),
                        reference.reference_id.as_str(),
                        reference.path.as_str(),
                        reference.name.as_str(),
                        &reference.line_range.start.to_string(),
                    ],
                ),
                file_id: reference.file_id.clone(),
                path: reference.path.clone(),
                caller_symbol_snapshot_id: caller_for_line(
                    &self.symbols,
                    &reference.path,
                    reference.line_range.start,
                )
                .map(|symbol| symbol.symbol_snapshot_id.clone()),
                caller_name: caller_for_line(
                    &self.symbols,
                    &reference.path,
                    reference.line_range.start,
                )
                .map(|symbol| symbol.name.clone()),
                callee_symbol_snapshot_id: reference.target_symbol_snapshot_id.clone(),
                callee_name: reference.name.clone(),
                line_range: reference.line_range.clone(),
            })
            .collect();

        CodeIndexSnapshot {
            repository_id: self.repository_id,
            resolved_commit_sha: self.commit,
            tree_hash: self.tree_hash,
            full_replace: self.full_replace,
            changed_path_count: self.changed_path_count,
            skipped_unchanged_count: self.skipped_unchanged_count,
            deleted_paths: self.deleted_paths,
            tombstones: self.tombstones,
            files: self.files,
            symbols: self.symbols,
            references: self.references,
            imports: self.imports,
            calls: self.calls,
            chunks: self.chunks,
            diagnostics: self.diagnostics,
        }
    }
}

fn resolve_reference_targets(
    symbols: &[RepositoryCodeSymbolRecord],
    references: &mut [RepositoryCodeReferenceRecord],
) {
    let mut by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        by_name.entry(&symbol.name).or_default().push(symbol);
    }
    for reference in references {
        reference.target_symbol_snapshot_id = resolve_reference_target(
            reference,
            by_name
                .get(reference.name.as_str())
                .map(std::vec::Vec::as_slice),
        )
        .map(|symbol| symbol.symbol_snapshot_id.clone());
    }
}

fn resolve_reference_target<'a>(
    reference: &RepositoryCodeReferenceRecord,
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Option<&'a RepositoryCodeSymbolRecord> {
    let candidates = candidates?;
    if candidates.len() == 1 {
        return candidates.first().copied();
    }

    let same_path = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.path == reference.path)
        .collect::<Vec<_>>();
    if same_path.len() == 1 {
        return same_path.first().copied();
    }

    None
}

fn caller_for_line<'a>(
    symbols: &'a [RepositoryCodeSymbolRecord],
    path: &str,
    line: u32,
) -> Option<&'a RepositoryCodeSymbolRecord> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.path == path && symbol.line_range.start <= line && symbol.line_range.end >= line
        })
        .max_by_key(|symbol| symbol.line_range.start)
}

fn resolve_git_root(path: &Path) -> Result<PathBuf, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["rev-parse".to_owned(), "--show-toplevel".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    Ok(PathBuf::from(root))
}

fn resolve_ref(root: &Path, ref_selector: &str) -> Result<String, CodeIndexError> {
    validate_git_ref_arg("ref_selector", ref_selector)?;
    git_text(
        root,
        ["rev-parse", "--verify", "--end-of-options", ref_selector],
    )
}

fn resolve_tree(root: &Path, commit: &str) -> Result<String, CodeIndexError> {
    git_text(root, ["rev-parse", &format!("{commit}^{{tree}}")])
}

fn tracked_paths(root: &Path, commit: &str) -> Result<Vec<String>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-r", "-z", "--name-only", commit])?;

    Ok(split_nul(&bytes))
}

fn diff_changes(
    root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    validate_git_ref_arg("base_ref", base_ref)?;
    validate_git_ref_arg("head_ref", head_ref)?;
    let bytes = git_bytes(
        root,
        [
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            "--end-of-options",
            base_ref,
            head_ref,
            "--",
        ],
    )?;

    parse_name_status_z(&bytes)
}

fn validate_git_ref_arg(field: &'static str, value: &str) -> Result<(), CodeIndexError> {
    if value.starts_with('-') {
        return Err(CodeIndexError::InvalidInput(format!(
            "{field} must not start with '-'"
        )));
    }

    Ok(())
}

fn git_text<const N: usize>(root: &Path, args: [&str; N]) -> Result<String, CodeIndexError> {
    let bytes = git_bytes(root, args)?;

    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

fn git_optional<const N: usize>(
    root: &Path,
    args: [&str; N],
) -> Result<Option<String>, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn git_bytes<const N: usize>(root: &Path, args: [&str; N]) -> Result<Vec<u8>, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        return Ok(output.stdout);
    }

    Err(CodeIndexError::Git {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn git_object_exists(root: &Path, object: &str) -> Result<bool, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", object])
        .output()?;

    Ok(output.status.success())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitChange {
    AddedOrModified { path: String },
    Deleted { path: String },
    Renamed { old_path: String, new_path: String },
    Copied { old_path: String, new_path: String },
    TypeChanged { path: String },
}

fn parse_name_status_z(bytes: &[u8]) -> Result<Vec<GitChange>, CodeIndexError> {
    let tokens = split_nul(bytes);
    let mut changes = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let status = &tokens[index];
        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            let old_path = tokens.get(index).cloned().ok_or_else(|| {
                CodeIndexError::InvalidInput("rename old path is missing".to_owned())
            })?;
            let new_path = tokens.get(index + 1).cloned().ok_or_else(|| {
                CodeIndexError::InvalidInput("rename new path is missing".to_owned())
            })?;
            index += 2;
            if status.starts_with('R') {
                changes.push(GitChange::Renamed { old_path, new_path });
            } else {
                changes.push(GitChange::Copied { old_path, new_path });
            }
            continue;
        }

        let path = tokens
            .get(index)
            .cloned()
            .ok_or_else(|| CodeIndexError::InvalidInput("changed path is missing".to_owned()))?;
        index += 1;
        match status.chars().next() {
            Some('D') => changes.push(GitChange::Deleted { path }),
            Some('T') => changes.push(GitChange::TypeChanged { path }),
            Some('A' | 'M') => changes.push(GitChange::AddedOrModified { path }),
            _ => changes.push(GitChange::AddedOrModified { path }),
        }
    }

    Ok(changes)
}

fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreePathChange {
    status: String,
    path: String,
    deleted_source: Option<String>,
}

impl WorktreePathChange {
    fn is_untracked(&self) -> bool {
        self.status == "??"
    }
}

fn worktree_changed_paths(status: &[u8]) -> Vec<WorktreePathChange> {
    let tokens = split_nul(status);
    let mut changes = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.len() < 4 {
            index += 1;
            continue;
        }
        let status = &token[..2];
        let path = token[3..].to_owned();
        if (status.contains('R') || status.contains('C')) && tokens.get(index + 1).is_some() {
            let source = tokens[index + 1].clone();
            changes.push(WorktreePathChange {
                status: status.to_owned(),
                path,
                deleted_source: status.contains('R').then_some(source),
            });
            index += 2;
            continue;
        }
        changes.push(WorktreePathChange {
            status: status.to_owned(),
            path,
            deleted_source: None,
        });
        index += 1;
    }

    changes
}

pub(super) fn stable_content_hash(bytes: &[u8]) -> String {
    format!("{:016x}", stable_hash64(bytes))
}

pub(super) fn stable_id<'a>(prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }

    format!("{prefix}:{:016x}", stable_hash64(&bytes))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
