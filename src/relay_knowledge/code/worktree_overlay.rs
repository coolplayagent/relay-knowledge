use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    CodeIndexError, MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, changes,
    filesystem_delta::build_filesystem_delta_snapshot,
    full_snapshot::build_full_snapshot,
    git::{git_bytes, resolve_ref},
    ids::{stable_content_hash, stable_hash64},
    parser::parse_indexed_file,
    scope,
    snapshot::SnapshotBuild,
    source::{source_commit_is_filesystem, source_kind},
    source_gitlink,
};

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

pub(super) fn build_worktree_overlay_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    previous_hashes: &BTreeMap<String, String>,
    base_resolved_commit_sha: Option<&str>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(&selector.ref_selector)
        || base_resolved_commit_sha.is_some_and(source_commit_is_filesystem)
        || source_kind(root)?.is_filesystem()
    {
        return build_filesystem_delta_snapshot(
            registration,
            selector,
            root,
            &selector.ref_selector,
            previous_hashes,
            base_resolved_commit_sha,
        );
    }
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
    let changes = changes::worktree_changed_paths(&status);
    if changes.is_empty() {
        return build_full_snapshot(registration, selector, root);
    }
    let mut overlay_hash_input = Vec::new();
    let mut deleted_paths = Vec::new();
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    for change in &changes {
        if let Some(deleted_path) = &change.deleted_source {
            if scope::path_scope_overlaps(deleted_path, registration, selector)
                && !record_deleted_gitlink_overlay(
                    root,
                    &commit,
                    deleted_path,
                    registration,
                    selector,
                    &mut overlay_hash_input,
                    &mut deleted_paths,
                )?
                && scope::path_is_selected(deleted_path, registration, selector)
            {
                record_worktree_deleted_path(
                    deleted_path,
                    &mut overlay_hash_input,
                    &mut deleted_paths,
                );
            }
        }
        let path = &change.path;
        if !scope::path_scope_overlaps(path, registration, selector) {
            continue;
        }
        if change.is_untracked()
            && !worktree_untracked_path_is_selected(path, registration, selector)
        {
            continue;
        }
        {
            let mut recorder = WorktreeOverlayRecorder {
                registration,
                selector,
                previous_hashes,
                overlay_hash_input: &mut overlay_hash_input,
                deleted_paths: &mut deleted_paths,
                files_to_parse: &mut files_to_parse,
                skipped_unchanged_count: &mut skipped_unchanged_count,
            };
            if record_staged_gitlink_overlay(change, root, &commit, &mut recorder)? {
                continue;
            }
        }
        let full_path = root.join(path);
        let metadata = match fs::symlink_metadata(&full_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if record_deleted_gitlink_overlay(
                    root,
                    &commit,
                    path,
                    registration,
                    selector,
                    &mut overlay_hash_input,
                    &mut deleted_paths,
                )? {
                    continue;
                } else if scope::path_is_selected(path, registration, selector) {
                    record_worktree_deleted_path(path, &mut overlay_hash_input, &mut deleted_paths);
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
            if contains_git_metadata(root, Path::new(path))? {
                let mut recorder = WorktreeOverlayRecorder {
                    registration,
                    selector,
                    previous_hashes,
                    overlay_hash_input: &mut overlay_hash_input,
                    deleted_paths: &mut deleted_paths,
                    files_to_parse: &mut files_to_parse,
                    skipped_unchanged_count: &mut skipped_unchanged_count,
                };
                record_unstaged_gitlink_overlay(root, &commit, path, &mut recorder)?;
                continue;
            }
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

fn record_worktree_deleted_path(
    path: &str,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) {
    overlay_hash_input.extend_from_slice(b"D\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
    deleted_paths.push(path.to_owned());
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

fn record_deleted_gitlink_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) -> Result<bool, CodeIndexError> {
    let Some(base_gitlink_commit) =
        source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?
    else {
        return Ok(false);
    };
    let entries = bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        &base_gitlink_commit,
        registration,
        selector,
    )?;
    if entries.is_empty() {
        if submodule_path_scope_overlaps(path, registration, selector) {
            record_worktree_status_marker(path, overlay_hash_input);
        }
        return Ok(true);
    }
    for entry in entries {
        if scope::path_is_selected(&entry.parent_path, registration, selector) {
            record_worktree_deleted_path(&entry.parent_path, overlay_hash_input, deleted_paths);
        }
    }

    Ok(true)
}

struct WorktreeOverlayRecorder<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    previous_hashes: &'a BTreeMap<String, String>,
    overlay_hash_input: &'a mut Vec<u8>,
    deleted_paths: &'a mut Vec<String>,
    files_to_parse: &'a mut Vec<(String, Vec<u8>)>,
    skipped_unchanged_count: &'a mut usize,
}

impl WorktreeOverlayRecorder<'_> {
    fn path_is_selected(&self, path: &str) -> bool {
        scope::path_is_selected(path, self.registration, self.selector)
    }

    fn record_deleted_path(&mut self, path: &str) {
        record_worktree_deleted_path(path, self.overlay_hash_input, self.deleted_paths);
    }

    fn record_gitlink_file(
        &mut self,
        root: &Path,
        submodule_path: &str,
        commit: &str,
        entry: &source_gitlink::SubmodulePathEntry,
    ) -> Result<(), CodeIndexError> {
        let bytes =
            source_gitlink::submodule_entry_bytes(root, submodule_path, commit, &entry.child_path)?;
        let blob_hash = stable_content_hash(&bytes);
        self.overlay_hash_input.extend_from_slice(b"F\0");
        self.overlay_hash_input
            .extend_from_slice(entry.parent_path.as_bytes());
        self.overlay_hash_input.push(0);
        self.overlay_hash_input
            .extend_from_slice(blob_hash.as_bytes());
        self.overlay_hash_input.push(0);
        if self.previous_hashes.get(&entry.parent_path) == Some(&blob_hash) {
            *self.skipped_unchanged_count += 1;
            return Ok(());
        }
        self.files_to_parse.push((entry.parent_path.clone(), bytes));

        Ok(())
    }
}

fn record_staged_gitlink_overlay(
    change: &changes::WorktreePathChange,
    root: &Path,
    base_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_>,
) -> Result<bool, CodeIndexError> {
    if !change.has_index_change() {
        return Ok(false);
    }
    let path = &change.path;
    let base_gitlink = source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?;
    let Some(staged_kind) = staged_path_kind(root, path)? else {
        if let Some(base_gitlink_commit) = base_gitlink {
            record_base_gitlink_child_deletions(
                root,
                path,
                base_commit,
                &base_gitlink_commit,
                recorder,
            )?;
            return Ok(true);
        }
        return Ok(false);
    };
    let StagedPathKind::Gitlink(staged_commit) = staged_kind else {
        if let Some(base_gitlink_commit) = base_gitlink {
            record_base_gitlink_child_deletions(
                root,
                path,
                base_commit,
                &base_gitlink_commit,
                recorder,
            )?;
        }
        return Ok(false);
    };

    if change.has_worktree_change()
        && let Some(worktree_commit) = submodule_worktree_head(root, path)?
        && worktree_commit != staged_commit
    {
        record_gitlink_commit_overlay(root, base_commit, path, &worktree_commit, recorder)?;
        return Ok(true);
    }

    record_gitlink_commit_overlay(root, base_commit, path, &staged_commit, recorder)?;

    Ok(true)
}

fn record_gitlink_commit_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    gitlink_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_>,
) -> Result<(), CodeIndexError> {
    let base_gitlink = source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?;
    let staged_entries = bounded_submodule_path_entries(
        root,
        path,
        None,
        gitlink_commit,
        recorder.registration,
        recorder.selector,
    )?;
    let staged_entries_are_empty = staged_entries.is_empty();
    if let Some(base_gitlink_commit) = base_gitlink {
        let staged_paths = staged_entries
            .iter()
            .map(|entry| entry.parent_path.clone())
            .collect::<BTreeSet<_>>();
        record_missing_base_gitlink_child_deletions(
            root,
            path,
            base_commit,
            &base_gitlink_commit,
            &staged_paths,
            recorder,
        )?;
    } else if base_path_exists(root, base_commit, path)? && recorder.path_is_selected(path) {
        recorder.record_deleted_path(path);
    }

    if staged_entries_are_empty
        && submodule_path_scope_overlaps(path, recorder.registration, recorder.selector)
    {
        record_worktree_status_marker(path, recorder.overlay_hash_input);
    }
    for entry in staged_entries {
        recorder.record_gitlink_file(root, path, gitlink_commit, &entry)?;
    }

    Ok(())
}

fn record_unstaged_gitlink_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    recorder: &mut WorktreeOverlayRecorder<'_>,
) -> Result<bool, CodeIndexError> {
    let Some(base_gitlink_commit) =
        source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?
    else {
        return Ok(false);
    };
    let Some(worktree_commit) = submodule_worktree_head(root, path)? else {
        return Ok(false);
    };
    if worktree_commit == base_gitlink_commit {
        return Ok(false);
    }

    let worktree_entries = bounded_submodule_path_entries(
        root,
        path,
        None,
        &worktree_commit,
        recorder.registration,
        recorder.selector,
    )?;
    let worktree_entries_are_empty = worktree_entries.is_empty();
    let worktree_paths = worktree_entries
        .iter()
        .map(|entry| entry.parent_path.clone())
        .collect::<BTreeSet<_>>();
    record_missing_base_gitlink_child_deletions(
        root,
        path,
        base_commit,
        &base_gitlink_commit,
        &worktree_paths,
        recorder,
    )?;
    if worktree_entries_are_empty
        && submodule_path_scope_overlaps(path, recorder.registration, recorder.selector)
    {
        record_worktree_status_marker(path, recorder.overlay_hash_input);
    }
    for entry in worktree_entries {
        recorder.record_gitlink_file(root, path, &worktree_commit, &entry)?;
    }

    Ok(true)
}

fn submodule_worktree_head(root: &Path, path: &str) -> Result<Option<String>, CodeIndexError> {
    let submodule_root = match source_gitlink::submodule_root(root, path) {
        Ok(submodule_root) => submodule_root,
        Err(_) => return Ok(None),
    };

    resolve_ref(&submodule_root, "HEAD").map(Some)
}

enum StagedPathKind {
    Gitlink(String),
    Regular,
}

fn staged_path_kind(root: &Path, path: &str) -> Result<Option<StagedPathKind>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-files", "-s", "-z", "--", path])?;
    let Some(record) = bytes
        .split(|byte| *byte == 0)
        .find(|record| !record.is_empty())
    else {
        return Ok(None);
    };
    let record = String::from_utf8_lossy(record);
    let Some((metadata, _)) = record.split_once('\t') else {
        return Ok(None);
    };
    let fields = metadata.split_whitespace().collect::<Vec<_>>();
    if fields.first().copied() != Some("160000") {
        return Ok(Some(StagedPathKind::Regular));
    }

    Ok(fields
        .get(1)
        .map(|object| StagedPathKind::Gitlink((*object).to_owned())))
}

fn record_base_gitlink_child_deletions(
    root: &Path,
    path: &str,
    base_commit: &str,
    base_gitlink_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_>,
) -> Result<(), CodeIndexError> {
    for entry in bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        base_gitlink_commit,
        recorder.registration,
        recorder.selector,
    )? {
        recorder.record_deleted_path(&entry.parent_path);
    }

    Ok(())
}

fn record_missing_base_gitlink_child_deletions(
    root: &Path,
    path: &str,
    base_commit: &str,
    base_gitlink_commit: &str,
    staged_paths: &BTreeSet<String>,
    recorder: &mut WorktreeOverlayRecorder<'_>,
) -> Result<(), CodeIndexError> {
    for entry in bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        base_gitlink_commit,
        recorder.registration,
        recorder.selector,
    )? {
        if !staged_paths.contains(&entry.parent_path) {
            recorder.record_deleted_path(&entry.parent_path);
        }
    }

    Ok(())
}

fn base_path_exists(root: &Path, base_commit: &str, path: &str) -> Result<bool, CodeIndexError> {
    git_object_kind(root, base_commit, path).map(|kind| kind.is_some())
}

fn git_object_kind(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<String>, CodeIndexError> {
    match git_bytes(root, ["cat-file", "-t", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).trim().to_owned())),
        Err(_) => Ok(None),
    }
}

fn bounded_submodule_path_entries(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<Vec<source_gitlink::SubmodulePathEntry>, CodeIndexError> {
    let Some(child_filters) = submodule_child_scope_filters(path, registration, selector) else {
        return Ok(Vec::new());
    };
    let entries = match source_gitlink::submodule_path_entries_with_child_filters(
        root,
        path,
        parent_commit,
        commit,
        &child_filters,
    ) {
        Ok(entries) => entries,
        Err(error) if source_gitlink::submodule_expansion_is_unavailable(&error) => Vec::new(),
        Err(error) => return Err(error),
    };
    let selected_entries = entries
        .into_iter()
        .filter(|entry| scope::path_is_selected(&entry.parent_path, registration, selector))
        .collect::<Vec<_>>();
    if selected_entries.len() > MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            selected_entries.len()
        )));
    }

    Ok(selected_entries)
}

fn submodule_child_scope_filters(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Option<Vec<String>> {
    let filters =
        scope::intersect_path_filters(&registration.path_filters, &selector.path_filters)?;
    if filters.is_empty() {
        return Some(Vec::new());
    }
    let mut child_filters = Vec::new();
    let mut parent_scope_covers_submodule = false;
    let path = normalize_worktree_path(path);
    if path.is_empty() {
        return None;
    }
    let child_prefix = format!("{path}/");
    for filter in filters {
        let filter = normalize_worktree_path(&filter);
        if filter.is_empty()
            || filter == "."
            || filter == path
            || path.starts_with(&format!("{filter}/"))
        {
            parent_scope_covers_submodule = true;
            continue;
        }
        if let Some(child_filter) = filter.strip_prefix(&child_prefix)
            && !child_filter.is_empty()
        {
            child_filters.push(child_filter.to_owned());
        }
    }
    if child_filters.is_empty() && !parent_scope_covers_submodule {
        return None;
    }
    child_filters.sort();
    child_filters.dedup();

    Some(child_filters)
}

fn submodule_path_scope_overlaps(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    submodule_child_scope_filters(path, registration, selector).is_some()
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
