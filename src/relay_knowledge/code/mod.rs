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

mod parser;

use crate::domain::{
    CodeCallRecord, CodeFileFingerprint, CodeIndexMode, CodeIndexSnapshot, CodePathTombstone,
    RepositoryCodeReferenceRecord, CodeRepositoryRegistration, CodeRepositorySelector, RepositoryCodeSymbolRecord,
};

use parser::{language_id, parse_indexed_file};

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
    let origin = git_optional(&root, ["config", "--get", "remote.origin.url"])?
        .unwrap_or_else(|| root.display().to_string());
    let repository_id = format!("repo:{:016x}", stable_hash64(origin.as_bytes()));

    CodeRepositoryRegistration::new(
        repository_id,
        alias,
        root.display().to_string(),
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
    let mut paths = Vec::new();
    for change in changes {
        match change {
            GitChange::AddedOrModified { path }
            | GitChange::Deleted { path }
            | GitChange::TypeChanged { path } => paths.push(path),
            GitChange::Renamed { old_path, new_path }
            | GitChange::Copied { old_path, new_path } => {
                paths.push(old_path);
                paths.push(new_path);
            }
        }
    }
    paths.sort();
    paths.dedup();

    Ok(paths)
}

fn build_full_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let commit = resolve_ref(root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(root, &commit)?;
    let paths = tracked_paths(root, &commit)?
        .into_iter()
        .filter(|path| path_is_selected(path, registration, selector))
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
    let commit = resolve_ref(root, head_ref)?;
    let tree_hash = resolve_tree(root, &commit)?;
    let changes = diff_changes(root, base_ref, head_ref)?;
    let mut build = SnapshotBuild::new(registration, commit, tree_hash, false, changes.len(), 0);

    for change in changes {
        match change {
            GitChange::Deleted { path } => build.deleted_paths.push(path),
            GitChange::Renamed { old_path, new_path } => {
                build.deleted_paths.push(old_path.clone());
                build.tombstones.push(CodePathTombstone {
                    repository_id: registration.repository_id.clone(),
                    old_path,
                    new_path: Some(new_path.clone()),
                    base_ref: base_ref.to_owned(),
                    head_ref: head_ref.to_owned(),
                });
                parse_changed_path(
                    &mut build,
                    registration,
                    selector,
                    root,
                    &new_path,
                    previous_hashes,
                )?;
            }
            GitChange::Copied { old_path, new_path } => {
                build.tombstones.push(CodePathTombstone {
                    repository_id: registration.repository_id.clone(),
                    old_path,
                    new_path: Some(new_path.clone()),
                    base_ref: base_ref.to_owned(),
                    head_ref: head_ref.to_owned(),
                });
                parse_changed_path(
                    &mut build,
                    registration,
                    selector,
                    root,
                    &new_path,
                    previous_hashes,
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
    let status = git_bytes(root, ["status", "--porcelain=v1", "-z"])?;
    let tree_hash = format!("worktree:{:016x}", stable_hash64(&status));
    let paths = worktree_changed_paths(&status);
    let mut build = SnapshotBuild::new(
        registration,
        commit,
        tree_hash,
        false,
        paths.len(),
        previous_hashes.len(),
    );

    for path in paths {
        if !path_is_selected(&path, registration, selector) {
            continue;
        }
        let full_path = root.join(&path);
        if !full_path.exists() {
            build.deleted_paths.push(path);
            continue;
        }
        let bytes = fs::read(full_path)?;
        let blob_hash = stable_content_hash(&bytes);
        if previous_hashes.get(&path) == Some(&blob_hash) {
            build.skipped_unchanged_count += 1;
            continue;
        }
        parse_indexed_file(&mut build, &path, &bytes)?;
    }

    Ok(build.finish())
}

fn parse_changed_path(
    build: &mut SnapshotBuild,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    path: &str,
    previous_hashes: &BTreeMap<String, String>,
) -> Result<(), CodeIndexError> {
    if !path_is_selected(path, registration, selector) {
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

fn resolve_reference_targets(symbols: &[RepositoryCodeSymbolRecord], references: &mut [RepositoryCodeReferenceRecord]) {
    let mut by_name = BTreeMap::new();
    for symbol in symbols {
        by_name
            .entry(symbol.name.clone())
            .or_insert_with(|| symbol.symbol_snapshot_id.clone());
    }
    for reference in references {
        reference.target_symbol_snapshot_id = by_name.get(&reference.name).cloned();
    }
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
    git_text(root, ["rev-parse", "--verify", ref_selector])
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
    let bytes = git_bytes(
        root,
        [
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            base_ref,
            head_ref,
        ],
    )?;

    parse_name_status_z(&bytes)
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

fn worktree_changed_paths(status: &[u8]) -> Vec<String> {
    let tokens = split_nul(status);
    let mut paths = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.len() < 4 {
            index += 1;
            continue;
        }
        let status = &token[..2];
        let path = token[3..].to_owned();
        if status.contains('R') || status.contains('C') {
            if let Some(new_path) = tokens.get(index + 1) {
                paths.push(new_path.clone());
                index += 2;
                continue;
            }
        }
        paths.push(path);
        index += 1;
    }

    paths
}

fn path_is_selected(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    let path_filters = if selector.path_filters.is_empty() {
        &registration.path_filters
    } else {
        &selector.path_filters
    };
    let language_filters = if selector.language_filters.is_empty() {
        &registration.language_filters
    } else {
        &selector.language_filters
    };
    let path_ok = path_filters.is_empty()
        || path_filters
            .iter()
            .any(|filter| path == filter || path.starts_with(&format!("{filter}/")));
    let language_ok = language_filters.is_empty()
        || language_id(path)
            .map(|language| language_filters.iter().any(|filter| filter == language))
            .unwrap_or(false);

    path_ok && language_ok
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_languages_and_filters_paths() {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec!["src".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate");
        let selector =
            CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
                .expect("selector should validate");

        assert_eq!(language_id("src/lib.rs"), Some("rust"));
        assert!(path_is_selected("src/lib.rs", &registration, &selector));
        assert!(!path_is_selected("tests/lib.rs", &registration, &selector));
        assert!(!path_is_selected("src/app.py", &registration, &selector));
    }

    #[test]
    fn parses_git_name_status_for_rename_copy_and_delete() {
        let changes = parse_name_status_z(
            b"M\0src/lib.rs\0R100\0old.rs\0new.rs\0C100\0a.py\0b.py\0D\0gone.ts\0",
        )
        .expect("name-status should parse");

        assert_eq!(
            changes,
            vec![
                GitChange::AddedOrModified {
                    path: "src/lib.rs".to_owned()
                },
                GitChange::Renamed {
                    old_path: "old.rs".to_owned(),
                    new_path: "new.rs".to_owned()
                },
                GitChange::Copied {
                    old_path: "a.py".to_owned(),
                    new_path: "b.py".to_owned()
                },
                GitChange::Deleted {
                    path: "gone.ts".to_owned()
                }
            ]
        );
    }
}
