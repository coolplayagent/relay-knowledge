use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::sync::Mutex;

pub(in crate::code) use super::filesystem::{
    FileSystemScanPolicy, explicit_path_filter_opts_into_default_file_exclusion,
    filesystem_default_source_allows, normalize_path_filter, source_default_file_preset_excludes,
    source_path_has_indexable_content,
};
use super::{
    CodeIndexError,
    changes::{GitTreeEntry, TrackedEntryScope, tracked_entries_state_with_scope},
    git::{
        git_batch_blob_sizes, git_batch_blobs, git_bytes, git_optional, resolve_git_root,
        resolve_ref, resolve_tree,
    },
    ids::{stable_content_hash, stable_hash64, stable_id},
    languages::language_id,
    parser::dependency_manifest_language_ids,
    source_gitlink,
    source_paths::FILESYSTEM_BROAD_SEGMENTS,
};

const FILESYSTEM_SYNTHETIC_PREFIX: &str = "filesystem:";

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
pub(in crate::code) enum RepositorySourceKind {
    Git,
    FileSystem,
}

impl RepositorySourceKind {
    pub(in crate::code) const fn is_filesystem(self) -> bool {
        matches!(self, Self::FileSystem)
    }
}

#[derive(Debug, Clone)]
pub(in crate::code) struct RegistrationSource {
    pub(in crate::code) root: PathBuf,
    pub(in crate::code) identity: String,
}

#[derive(Debug, Clone)]
pub(in crate::code) struct RepositorySourceSnapshot {
    pub(in crate::code) kind: RepositorySourceKind,
    pub(in crate::code) root: PathBuf,
    pub(in crate::code) resolved_commit_sha: String,
    pub(in crate::code) tree_hash: String,
    pub(in crate::code) entries: Vec<GitTreeEntry>,
}

pub(in crate::code) fn registration_source(
    path: &Path,
) -> Result<RegistrationSource, CodeIndexError> {
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

pub(in crate::code) fn filesystem_registration_identity(
    path: &Path,
) -> Result<String, CodeIndexError> {
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

pub(in crate::code) fn source_kind(root: &Path) -> Result<RepositorySourceKind, CodeIndexError> {
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

pub(in crate::code) fn source_snapshot(
    root: &Path,
    ref_selector: &str,
    filesystem_policy: FileSystemScanPolicy,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    match source_kind(root)? {
        RepositorySourceKind::Git => {
            let commit = resolve_ref(root, ref_selector)?;
            let parent_tree_hash = resolve_tree(root, &commit)?;
            let entry_scope = if filesystem_policy.path_scope_denied {
                TrackedEntryScope::empty()
            } else {
                TrackedEntryScope::from_path_filters(filesystem_policy.path_scope_filters())
            };
            let tracked = tracked_entries_state_with_scope(root, &commit, &entry_scope)?;
            let tree_hash =
                git_tree_hash_with_submodules(&parent_tree_hash, &tracked.submodule_states);
            Ok(RepositorySourceSnapshot {
                kind: RepositorySourceKind::Git,
                root: root.to_path_buf(),
                resolved_commit_sha: commit,
                tree_hash,
                entries: tracked.entries,
            })
        }
        RepositorySourceKind::FileSystem => filesystem_source_snapshot(root, filesystem_policy),
    }
}

pub(in crate::code) fn git_tree_hash_with_submodules(
    parent_tree_hash: &str,
    submodule_states: &[String],
) -> String {
    if submodule_states.is_empty() {
        return parent_tree_hash.to_owned();
    }

    let mut hash_input = Vec::new();
    hash_input.extend_from_slice(b"git-tree-with-submodules-v1\0");
    hash_input.extend_from_slice(parent_tree_hash.as_bytes());
    hash_input.push(0);
    for state in submodule_states {
        hash_input.extend_from_slice(state.as_bytes());
        hash_input.push(0);
    }

    format!("git_tree:{:016x}", stable_hash64(&hash_input))
}

pub(in crate::code) fn filesystem_source_snapshot(
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
    match git_bytes(root, ["show", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(bytes),
        Err(error) => source_gitlink::submodule_bytes(root, commit, path).map_err(|_| error),
    }
}

pub(in crate::code) fn source_bytes_after_content_verification(
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

pub(in crate::code) fn source_snapshot_bytes(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    if kind.is_filesystem() {
        return filesystem_bytes(root, path);
    }

    source_bytes_after_policy_verification(root, commit, path)
}

fn source_batch_bytes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let sizes = match git_batch_blob_sizes(root, commit, paths) {
        Ok(sizes) => sizes,
        Err(_) => {
            return paths
                .iter()
                .map(|path| source_bytes_after_policy_verification(root, commit, path))
                .collect();
        }
    };

    let mut blobs = vec![None::<Vec<u8>>; paths.len()];
    let mut parent_blob_indices = Vec::new();
    let mut parent_blob_paths = Vec::new();
    for (index, (path, size)) in paths.iter().zip(sizes.iter()).enumerate() {
        if size.is_some() {
            parent_blob_indices.push(index);
            parent_blob_paths.push(path.clone());
        }
    }

    let parent_blobs = if parent_blob_paths.is_empty() {
        Vec::new()
    } else {
        match git_batch_blobs(root, commit, &parent_blob_paths) {
            Ok(blobs) => blobs,
            Err(_) => parent_blob_paths
                .iter()
                .map(|path| source_bytes_after_policy_verification(root, commit, path))
                .collect::<Result<Vec<_>, _>>()?,
        }
    };
    for (index, bytes) in parent_blob_indices.into_iter().zip(parent_blobs) {
        blobs[index] = Some(bytes);
    }
    for (index, path) in paths.iter().enumerate() {
        if blobs[index].is_none() {
            blobs[index] = Some(source_bytes_after_policy_verification(root, commit, path)?);
        }
    }

    blobs
        .into_iter()
        .map(|bytes| {
            bytes.ok_or_else(|| {
                CodeIndexError::InvalidInput(
                    "source batch bytes left a path without content".to_owned(),
                )
            })
        })
        .collect()
}

pub(in crate::code) fn source_batch_bytes_after_content_verification(
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

pub(in crate::code) fn source_snapshot_batch_bytes(
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

    source_batch_bytes_after_policy_verification(root, commit, paths)
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

pub(in crate::code) fn source_blob_sizes_after_policy_verification(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    if commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX) {
        return filesystem_blob_sizes(root, paths);
    }

    let mut sizes = match git_batch_blob_sizes(root, commit, paths) {
        Ok(sizes) => sizes,
        Err(_) => {
            return paths
                .iter()
                .map(|path| git_blob_size_after_policy_verification(root, commit, path))
                .collect();
        }
    };
    for (path, size) in paths.iter().zip(sizes.iter_mut()) {
        if size.is_none() {
            *size = source_gitlink::submodule_blob_size(root, commit, path)?;
        }
    }

    Ok(sizes)
}

fn git_blob_size_after_policy_verification(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    let object = format!("{commit}:{path}");
    match git_bytes(root, ["cat-file", "-s", &object]) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
        Err(_) => source_gitlink::submodule_blob_size(root, commit, path),
    }
}

pub(in crate::code) fn source_commit_is_filesystem(commit: &str) -> bool {
    commit.starts_with(FILESYSTEM_SYNTHETIC_PREFIX)
}

pub(in crate::code) fn source_language_filter_allows(path: &str, filters: &[String]) -> bool {
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

pub(in crate::code) fn filesystem_tree_hash_for_paths(
    root: &Path,
    paths: &[String],
) -> Result<String, CodeIndexError> {
    let path_hashes = filesystem_content_hashes_for_paths(root, paths)?;

    Ok(filesystem_tree_hash_from_path_hashes(&path_hashes))
}

pub(in crate::code) fn filesystem_content_hashes_for_paths(
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

pub(in crate::code) fn filesystem_tree_hash_from_path_hashes(
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

pub(in crate::code) fn ensure_filesystem_paths_match_content_hashes(
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

pub(in crate::code) fn ensure_filesystem_blobs_match_content_hashes(
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
    if policy.path_scope_denied {
        return Ok(Vec::new());
    }
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
