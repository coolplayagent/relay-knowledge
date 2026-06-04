use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::sync::Mutex;

use super::{
    CodeIndexError,
    changes::{GitTreeEntry, tracked_entries},
    git::{
        git_batch_blob_sizes, git_batch_blobs, git_bytes, git_optional, resolve_git_root,
        resolve_ref, resolve_tree,
    },
    ids::{stable_content_hash, stable_hash64, stable_id},
    languages::language_id,
    parser::{dependency_manifest_language_ids, dependency_manifest_overrides_default_exclusion},
    source_roots::STRIPPABLE_SOURCE_ROOTS,
};

const FILESYSTEM_SYNTHETIC_PREFIX: &str = "filesystem:";
const DEFAULT_EXCLUDED_EXTENSIONS: &[&str] = &[
    "7z", "avif", "bmp", "bz2", "class", "eot", "gif", "gz", "ico", "jar", "jpeg", "jpg", "jsonl",
    "lockb", "map", "mov", "mp4", "otf", "pdf", "png", "svg", "tar", "tgz", "ttf", "wasm", "webm",
    "woff", "woff2", "zip", "zst",
];
const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &["uv.lock"];
const FILESYSTEM_BROAD_SEGMENTS: &[&str] = &[
    ".cache",
    ".git",
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
const FILESYSTEM_DEFAULT_SOURCE_ROOTS: &[&str] = &[
    "app",
    "config",
    "configs",
    "docs",
    "extensions",
    "include",
    "lib",
    "modules",
    "packages",
    "plugins",
    "source",
    "Sources",
    "src",
];
const FILESYSTEM_AUTO_DISCOVERY_FILTERS: &[&str] = &["src", "include", "lib", "Sources"];

#[cfg(test)]
struct FileSystemPolicyReadMutation {
    root: PathBuf,
    path: String,
    content: Vec<u8>,
}

#[cfg(test)]
static FILESYSTEM_POLICY_READ_MUTATION: Mutex<Option<FileSystemPolicyReadMutation>> =
    Mutex::new(None);

#[cfg(test)]
pub(crate) fn mutate_next_filesystem_policy_read(root: PathBuf, path: &str, content: &[u8]) {
    *FILESYSTEM_POLICY_READ_MUTATION
        .lock()
        .expect("filesystem read mutation should lock") = Some(FileSystemPolicyReadMutation {
        root,
        path: path.to_owned(),
        content: content.to_vec(),
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepositorySourceKind {
    Git,
    FileSystem,
}

impl RepositorySourceKind {
    pub(super) const fn is_filesystem(self) -> bool {
        matches!(self, Self::FileSystem)
    }
}

#[derive(Debug, Clone)]
pub(super) struct RegistrationSource {
    pub(super) root: PathBuf,
    pub(super) identity: String,
}

#[derive(Debug, Clone)]
pub(super) struct RepositorySourceSnapshot {
    pub(super) kind: RepositorySourceKind,
    pub(super) root: PathBuf,
    pub(super) resolved_commit_sha: String,
    pub(super) tree_hash: String,
    pub(super) entries: Vec<GitTreeEntry>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct FileSystemScanPolicy {
    explicit_root_scope: bool,
    broad_directory_filters: Vec<String>,
    path_scope_filters: Vec<String>,
    language_filter_sets: Vec<Vec<String>>,
}

impl FileSystemScanPolicy {
    #[cfg(test)]
    pub(super) fn from_path_filters<'a>(
        path_filters: impl IntoIterator<Item = &'a String>,
    ) -> Self {
        Self::from_path_and_language_filters(path_filters, &[], &[])
    }

    pub(super) fn from_path_and_language_filters<'a>(
        path_filters: impl IntoIterator<Item = &'a String>,
        registration_language_filters: &[String],
        selector_language_filters: &[String],
    ) -> Self {
        let normalized = path_filters
            .into_iter()
            .map(|filter| normalize_path_filter(filter))
            .filter(|filter| !filter.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let language_filter_sets = [registration_language_filters, selector_language_filters]
            .into_iter()
            .filter(|filters| !filters.is_empty())
            .map(<[String]>::to_vec)
            .collect();

        Self {
            explicit_root_scope: normalized.iter().any(|filter| filter == "."),
            broad_directory_filters: normalized
                .iter()
                .filter(|filter| filter.as_str() != ".")
                .cloned()
                .collect(),
            path_scope_filters: normalized
                .into_iter()
                .filter(|filter| filter != ".")
                .collect(),
            language_filter_sets,
        }
    }

    fn includes_broad_directory(&self, directory: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }

        self.broad_directory_filters
            .iter()
            .any(|filter| filter == directory || filter.starts_with(&format!("{directory}/")))
    }

    fn hash_includes_path(&self, path: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }
        if self.path_scope_filters.is_empty() {
            return filesystem_default_source_allows(path);
        }

        self.path_scope_filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
    }

    fn file_preset_allows_hash(&self, path: &str) -> bool {
        !source_default_file_preset_excludes(path)
            || dependency_manifest_overrides_default_exclusion(path)
            || explicit_path_filter_opts_into_default_file_exclusion(
                path,
                self.path_scope_filters.iter(),
            )
    }

    fn language_allows_hash(&self, path: &str) -> bool {
        self.language_filter_sets
            .iter()
            .all(|filters| source_language_filter_allows(path, filters))
    }

    pub(super) fn should_descend_directory(&self, directory: &str) -> bool {
        if self.explicit_root_scope {
            return true;
        }
        if self.path_scope_filters.is_empty() {
            return filesystem_default_directory_can_contribute(directory);
        }

        if self
            .path_scope_filters
            .iter()
            .any(|filter| path_overlaps_filter(directory, filter))
        {
            return true;
        }

        self.path_scope_filters
            .iter()
            .any(|filter| filesystem_filter_can_discover_roots(filter))
            && discoverable_source_directory(directory)
    }
}

pub(super) fn registration_source(path: &Path) -> Result<RegistrationSource, CodeIndexError> {
    match resolve_git_root(path) {
        Ok(root) => {
            let root_identity = root.display().to_string();
            let origin = git_optional(&root, ["config", "--get", "remote.origin.url"])?
                .unwrap_or_else(|| root_identity.clone());
            Ok(RegistrationSource {
                root,
                identity: stable_id("repo", [origin.as_str(), root_identity.as_str()]),
            })
        }
        Err(git_error) => {
            if !git_error_is_not_repository(&git_error) || path_or_parent_has_git_metadata(path)? {
                return Err(git_error);
            }
            let root = path.canonicalize().map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => git_error,
                _ => CodeIndexError::Io(error),
            })?;
            if !root.is_dir() {
                return Err(CodeIndexError::InvalidInput(format!(
                    "code repository root '{}' is not a directory",
                    root.display()
                )));
            }
            Ok(RegistrationSource {
                identity: filesystem_registration_identity_for_root(&root),
                root,
            })
        }
    }
}

pub(super) fn filesystem_registration_identity(path: &Path) -> Result<String, CodeIndexError> {
    let root = path.canonicalize()?;
    if !root.is_dir() {
        return Err(CodeIndexError::InvalidInput(format!(
            "code repository root '{}' is not a directory",
            root.display()
        )));
    }

    Ok(filesystem_registration_identity_for_root(&root))
}

fn filesystem_registration_identity_for_root(root: &Path) -> String {
    let root_identity = root.display().to_string();
    stable_id("repo", ["filesystem", root_identity.as_str()])
}

pub(super) fn source_kind(root: &Path) -> Result<RepositorySourceKind, CodeIndexError> {
    match resolve_git_root(root) {
        Ok(_) => Ok(RepositorySourceKind::Git),
        Err(error)
            if git_error_is_not_repository(&error) && !path_or_parent_has_git_metadata(root)? =>
        {
            Ok(RepositorySourceKind::FileSystem)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn source_snapshot(
    root: &Path,
    ref_selector: &str,
    filesystem_policy: FileSystemScanPolicy,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    match source_kind(root)? {
        RepositorySourceKind::Git => {
            let commit = resolve_ref(root, ref_selector)?;
            let tree_hash = resolve_tree(root, &commit)?;
            let entries = tracked_entries(root, &commit)?;
            Ok(RepositorySourceSnapshot {
                kind: RepositorySourceKind::Git,
                root: root.to_path_buf(),
                resolved_commit_sha: commit,
                tree_hash,
                entries,
            })
        }
        RepositorySourceKind::FileSystem => filesystem_source_snapshot(root, filesystem_policy),
    }
}

pub(super) fn filesystem_source_snapshot(
    root: &Path,
    policy: FileSystemScanPolicy,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    let root = root.canonicalize()?;
    let files = filesystem_files(&root, &policy)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut hash_paths = Vec::new();
    for file in files {
        let byte_count = filesystem_byte_count(&root, &file.path)?;
        if policy.hash_includes_path(&file.path)
            && policy.language_allows_hash(&file.path)
            && policy.file_preset_allows_hash(&file.path)
        {
            hash_paths.push(file.path.clone());
        }
        entries.push(GitTreeEntry {
            path: file.path,
            byte_count,
        });
    }
    let tree_hash = filesystem_tree_hash_for_paths(&root, &hash_paths)?;

    Ok(RepositorySourceSnapshot {
        kind: RepositorySourceKind::FileSystem,
        root,
        resolved_commit_sha: tree_hash.clone(),
        tree_hash,
        entries,
    })
}

fn source_bytes_after_policy_verification(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    git_bytes(root, ["show", &format!("{commit}:{path}")])
}

pub(super) fn source_bytes_after_content_verification(
    root: &Path,
    commit: &str,
    path: &str,
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<Vec<u8>, CodeIndexError> {
    if commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX) {
        let paths = [path.to_owned()];
        let blobs =
            source_batch_bytes_after_content_verification(root, commit, &paths, expected_hashes)?;
        return blobs.into_iter().next().ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} produced no bytes for {path}"
            ))
        });
    }

    source_bytes_after_policy_verification(root, commit, path)
}

pub(super) fn source_snapshot_bytes(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    if kind.is_filesystem() {
        return filesystem_bytes(root, path);
    }

    git_bytes(root, ["show", &format!("{commit}:{path}")])
}

fn source_batch_bytes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    git_batch_blobs(root, commit, paths)
}

pub(super) fn source_batch_bytes_after_content_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX) {
        let expected_hashes = expected_hashes.ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing verified content hashes"
            ))
        })?;
        return filesystem_batch_bytes_after_hash_check(root, commit, paths, expected_hashes);
    }

    source_batch_bytes_after_policy_verification(root, commit, paths)
}

pub(super) fn source_snapshot_batch_bytes(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if kind.is_filesystem() {
        return paths
            .iter()
            .map(|path| filesystem_bytes(root, path))
            .collect();
    }

    git_batch_blobs(root, commit, paths)
}

fn filesystem_blob_sizes(
    root: &Path,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    paths
        .iter()
        .map(|path| {
            let full_path = safe_filesystem_path(root, path)?;
            Ok(fs::metadata(full_path)
                .ok()
                .map(|metadata| usize::try_from(metadata.len()).unwrap_or(usize::MAX)))
        })
        .collect()
}

pub(super) fn source_blob_sizes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    if commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX) {
        return filesystem_blob_sizes(root, paths);
    }

    git_batch_blob_sizes(root, commit, paths)
}

pub(super) fn source_commit_is_filesystem(commit: &str) -> bool {
    commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX)
}

pub(super) fn source_language_filter_allows(path: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    if language_id(path).is_some_and(|language| {
        filters.iter().any(|filter| {
            filter == language
                || cxx_header_filter_allows(path, language, filter)
                || unknown_filter_allows_document_path(path, language, filter)
        })
    }) {
        return true;
    }
    dependency_manifest_language_ids(path).is_some_and(|languages| {
        languages
            .iter()
            .any(|language| filters.iter().any(|filter| filter == language))
    })
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

fn unknown_filter_allows_document_path(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "unknown" && document_like_language_path(path, language_id)
}

fn document_like_language_path(path: &str, language_id: &str) -> bool {
    matches!(
        language_id,
        "markdown" | "json" | "yaml" | "toml" | "xml" | "ini" | "properties"
    ) || matches!(
        path.rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

pub(super) fn filesystem_tree_hash_for_paths(
    root: &Path,
    paths: &[String],
) -> Result<String, CodeIndexError> {
    let path_hashes = filesystem_content_hashes_for_paths(root, paths)?;

    Ok(filesystem_tree_hash_from_path_hashes(&path_hashes))
}

pub(super) fn filesystem_content_hashes_for_paths(
    root: &Path,
    paths: &[String],
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    let root = root.canonicalize()?;
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut path_hashes = BTreeMap::new();
    for path in paths {
        path_hashes.insert(path.clone(), filesystem_content_hash(&root, &path)?);
    }

    Ok(path_hashes)
}

pub(super) fn filesystem_tree_hash_from_path_hashes(
    path_hashes: &BTreeMap<String, String>,
) -> String {
    let mut hash_input = Vec::new();
    for (path, content_hash) in path_hashes {
        hash_input.extend_from_slice(path.as_bytes());
        hash_input.push(0);
        hash_input.extend_from_slice(content_hash.as_bytes());
        hash_input.push(0);
    }

    format!(
        "{FILESYSTEM_SYNTHETIC_PREFIX}{:016x}",
        stable_hash64(&hash_input)
    )
}

pub(super) fn ensure_filesystem_paths_match_content_hashes(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<(), CodeIndexError> {
    if !source_commit_is_filesystem(commit) {
        return Ok(());
    }
    let root = root.canonicalize()?;
    for path in paths {
        let expected_hash = expected_hashes.get(path).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing planned content hash for {path}"
            ))
        })?;
        let actual_hash = filesystem_content_hash(&root, path)?;
        if &actual_hash != expected_hash {
            return Err(CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} no longer matches planned filesystem file {path}"
            )));
        }
    }

    Ok(())
}

pub(super) fn ensure_filesystem_blobs_match_content_hashes(
    commit: &str,
    paths: &[String],
    blobs: &[Vec<u8>],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<(), CodeIndexError> {
    if !source_commit_is_filesystem(commit) {
        return Ok(());
    }
    for (path, bytes) in paths.iter().zip(blobs) {
        let expected_hash = expected_hashes.get(path).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} is missing planned content hash for {path}"
            ))
        })?;
        let actual_hash = stable_content_hash(bytes);
        if &actual_hash != expected_hash {
            return Err(CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {commit} no longer matches planned filesystem file {path}"
            )));
        }
    }

    Ok(())
}

fn filesystem_bytes(root: &Path, path: &str) -> Result<Vec<u8>, CodeIndexError> {
    fs::read(safe_filesystem_path(root, path)?).map_err(CodeIndexError::Io)
}

fn filesystem_batch_bytes_after_hash_check(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: &BTreeMap<String, String>,
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    #[cfg(test)]
    apply_filesystem_policy_read_mutation(root)?;
    let blobs = paths
        .iter()
        .map(|path| filesystem_bytes(root, path))
        .collect::<Result<Vec<_>, _>>()?;
    ensure_filesystem_blobs_match_content_hashes(commit, paths, &blobs, expected_hashes)?;

    Ok(blobs)
}

#[cfg(test)]
fn apply_filesystem_policy_read_mutation(root: &Path) -> Result<(), CodeIndexError> {
    let mut mutation = FILESYSTEM_POLICY_READ_MUTATION
        .lock()
        .expect("filesystem read mutation should lock");
    let Some(next) = mutation.take() else {
        return Ok(());
    };
    if next.root != root {
        *mutation = Some(next);
        return Ok(());
    }

    fs::write(root.join(next.path), next.content).map_err(CodeIndexError::Io)
}

fn filesystem_content_hash(root: &Path, path: &str) -> Result<String, CodeIndexError> {
    fs::read(safe_filesystem_path(root, path)?)
        .map(|bytes| stable_content_hash(&bytes))
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CodeIndexError::InvalidInput(format!(
                "filesystem source path {path} is missing from live source tree"
            )),
            _ => CodeIndexError::Io(error),
        })
}

fn filesystem_byte_count(root: &Path, path: &str) -> Result<usize, CodeIndexError> {
    fs::metadata(safe_filesystem_path(root, path)?)
        .map(|metadata| usize::try_from(metadata.len()).unwrap_or(usize::MAX))
        .map_err(CodeIndexError::Io)
}

fn safe_filesystem_path(root: &Path, path: &str) -> Result<PathBuf, CodeIndexError> {
    if !safe_relative_path(path) {
        return Err(CodeIndexError::InvalidInput(format!(
            "unsafe repository source path '{path}'"
        )));
    }

    let mut checked_path = root.to_path_buf();
    let mut checked_relative = PathBuf::new();
    for component in Path::new(path).components() {
        checked_path.push(component.as_os_str());
        checked_relative.push(component.as_os_str());
        match fs::symlink_metadata(&checked_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(CodeIndexError::InvalidInput(format!(
                    "filesystem source path {path} component {} is a symlink and is outside the authorized regular-file scope",
                    checked_relative.to_string_lossy()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(root.join(path));
            }
            Err(error) => return Err(CodeIndexError::Io(error)),
        }
    }

    let full_path = checked_path;
    match fs::symlink_metadata(&full_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CodeIndexError::InvalidInput(format!(
                "filesystem source path {path} is a symlink and is outside the authorized regular-file scope"
            )))
        }
        Ok(_) => Ok(full_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(full_path),
        Err(error) => Err(CodeIndexError::Io(error)),
    }
}

#[derive(Debug, Clone)]
struct FileSystemFile {
    path: String,
}

fn filesystem_files(
    root: &Path,
    policy: &FileSystemScanPolicy,
) -> Result<Vec<FileSystemFile>, CodeIndexError> {
    let mut files = Vec::new();
    collect_files(root, Path::new(""), policy, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|left, right| left.path == right.path);

    Ok(files)
}

fn collect_files(
    root: &Path,
    relative: &Path,
    policy: &FileSystemScanPolicy,
    files: &mut Vec<FileSystemFile>,
) -> Result<(), CodeIndexError> {
    let mut entries = fs::read_dir(root.join(relative))?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let directory = path.to_string_lossy().replace('\\', "/");
            if directory_is_excluded(&path, policy)
                || !policy.should_descend_directory(&directory)
                || contains_git_metadata(root, &path)?
            {
                continue;
            }
            collect_files(root, &path, policy, files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = path.to_string_lossy().replace('\\', "/");
        if safe_relative_path(&path) {
            files.push(FileSystemFile { path });
        }
    }

    Ok(())
}

fn directory_is_excluded(relative: &Path, policy: &FileSystemScanPolicy) -> bool {
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name == ".git" {
        return true;
    }
    if !FILESYSTEM_BROAD_SEGMENTS.contains(&name) {
        return false;
    }
    let directory = relative.to_string_lossy().replace('\\', "/");
    let directory = normalize_path_filter(&directory);

    !policy.includes_broad_directory(directory)
}

fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn filesystem_default_source_allows(path: &str) -> bool {
    let normalized = normalize_path_filter(path);
    if normalized
        .split('/')
        .any(|segment| FILESYSTEM_BROAD_SEGMENTS.contains(&segment))
    {
        return false;
    }
    if !source_path_has_indexable_content(normalized) {
        return false;
    }
    let mut segments = normalized.split('/');
    let Some(first) = segments.next() else {
        return false;
    };
    if segments.next().is_none() {
        return true;
    }

    FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(&first)
}

fn filesystem_default_directory_can_contribute(directory: &str) -> bool {
    let directory = normalize_path_filter(directory);
    let Some(first) = directory.split('/').next() else {
        return false;
    };

    FILESYSTEM_DEFAULT_SOURCE_ROOTS.contains(&first)
}

pub(super) fn source_path_has_indexable_content(path: &str) -> bool {
    language_id(path).is_some() || dependency_manifest_language_ids(path).is_some()
}

pub(super) fn source_default_file_preset_excludes(path: &str) -> bool {
    let normalized = normalize_path_filter(path);
    if normalized
        .rsplit('/')
        .next()
        .is_some_and(|file_name| DEFAULT_EXCLUDED_FILENAMES.contains(&file_name))
    {
        return true;
    }
    normalized
        .rsplit_once('.')
        .map(|(_, extension)| {
            DEFAULT_EXCLUDED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
        .unwrap_or(false)
}

pub(super) fn explicit_path_filter_opts_into_default_file_exclusion<'a>(
    path: &str,
    filters: impl IntoIterator<Item = &'a String>,
) -> bool {
    let path_extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    filters.into_iter().any(|filter| {
        let filter = normalize_path_filter(filter);
        if filter.is_empty() || filter == "." {
            return false;
        }
        let filter_segments = filter.split('/').collect::<Vec<_>>();
        let targets_default_exclusion = filter_segments.iter().any(|segment| {
            DEFAULT_EXCLUDED_FILENAMES.contains(segment)
                || segment
                    .rsplit_once('.')
                    .map(|(_, ext)| ext.to_ascii_lowercase())
                    .is_some_and(|extension| {
                        DEFAULT_EXCLUDED_EXTENSIONS.contains(&extension.as_str())
                    })
        });
        if !targets_default_exclusion {
            return false;
        }
        path_matches_filter(path, filter)
            || filter.strip_prefix("*.").is_some_and(|extension| {
                path_extension.as_deref() == Some(&extension.to_ascii_lowercase())
            })
    })
}

fn git_error_is_not_repository(error: &CodeIndexError) -> bool {
    matches!(error, CodeIndexError::Git { message, .. } if message.contains("not a git repository"))
}

fn path_or_parent_has_git_metadata(path: &Path) -> Result<bool, CodeIndexError> {
    let Ok(mut current) = path.canonicalize() else {
        return Ok(false);
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        match fs::symlink_metadata(current.join(".git")) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if !current.pop() {
            return Ok(false);
        }
    }
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim().trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

fn path_overlaps_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !path.is_empty()
        && !filter.is_empty()
        && (path == filter
            || path.starts_with(&format!("{filter}/"))
            || filter.starts_with(&format!("{path}/")))
}

fn filesystem_filter_can_discover_roots(filter: &str) -> bool {
    let filter = normalize_path_filter(filter);
    FILESYSTEM_AUTO_DISCOVERY_FILTERS.contains(&filter)
}

fn discoverable_source_directory(directory: &str) -> bool {
    let directory = normalize_path_filter(directory);
    let Some(first_segment) = directory.split('/').next() else {
        return false;
    };
    if FILESYSTEM_AUTO_DISCOVERY_FILTERS.contains(&first_segment) {
        return true;
    }
    STRIPPABLE_SOURCE_ROOTS
        .iter()
        .map(|root| root.trim_end_matches('/'))
        .any(|root| root == first_segment)
}
