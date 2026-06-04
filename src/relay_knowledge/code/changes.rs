use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[cfg(test)]
use std::sync::Mutex;

use super::{
    CodeIndexError,
    git::{git_bytes, git_bytes_slice, resolve_git_root, validate_git_ref_arg},
};

const MAX_SUBMODULE_EXPANSION_DEPTH: usize = 8;

#[cfg(test)]
static TRACKED_ENTRIES_OBSERVER: Mutex<Option<(PathBuf, usize)>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn reset_tracked_entries_call_count_for_root(root: PathBuf) {
    *TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock") = Some((root, 0));
}

#[cfg(test)]
pub(crate) fn tracked_entries_call_count_for_root(root: &Path) -> usize {
    TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock")
        .as_ref()
        .filter(|(observed_root, _)| observed_root == root)
        .map(|(_, count)| *count)
        .unwrap_or(0)
}

#[cfg(test)]
pub(super) fn tracked_entries(
    root: &Path,
    commit: &str,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    tracked_entries_with_scope(root, commit, &TrackedEntryScope::all())
}

pub(super) fn tracked_entries_with_scope(
    root: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    Ok(tracked_entries_state_with_scope(root, commit, scope)?.entries)
}

pub(super) fn tracked_entries_state_with_scope(
    root: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let mut visited = BTreeSet::new();
    tracked_entries_inner(root, commit, "", 0, &mut visited, scope)
}

#[derive(Debug, Clone, Default)]
pub(super) struct GitTrackedEntries {
    pub(super) entries: Vec<GitTreeEntry>,
    pub(super) submodule_states: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct TrackedEntryScope {
    path_filters: Vec<String>,
    entry_filter: TrackedEntryFilter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TrackedEntryFilter {
    #[default]
    None,
    Empty,
    Nested,
    All,
}

impl TrackedEntryScope {
    #[cfg(test)]
    pub(super) fn all() -> Self {
        Self {
            path_filters: Vec::new(),
            entry_filter: TrackedEntryFilter::None,
        }
    }

    pub(super) fn empty() -> Self {
        Self {
            path_filters: Vec::new(),
            entry_filter: TrackedEntryFilter::Empty,
        }
    }

    pub(super) fn from_path_filters<'a>(filters: impl IntoIterator<Item = &'a String>) -> Self {
        Self {
            path_filters: filters
                .into_iter()
                .map(|filter| normalize_path_filter(filter).to_owned())
                .filter(|filter| !filter.is_empty())
                .collect(),
            entry_filter: TrackedEntryFilter::Nested,
        }
    }

    pub(super) fn from_entry_path_filters<'a>(
        filters: impl IntoIterator<Item = &'a String>,
    ) -> Self {
        Self {
            path_filters: filters
                .into_iter()
                .map(|filter| normalize_path_filter(filter).to_owned())
                .filter(|filter| !filter.is_empty())
                .collect(),
            entry_filter: TrackedEntryFilter::All,
        }
    }

    fn allows_submodule_expansion(&self, path: &str) -> bool {
        self.entry_filter != TrackedEntryFilter::Empty
            && (self.path_filters.is_empty()
                || self
                    .path_filters
                    .iter()
                    .any(|filter| path_overlaps_filter(path, filter)))
    }

    fn allows_entry(&self, prefix: &str, path: &str) -> bool {
        let path = format!("{prefix}{path}");
        match self.entry_filter {
            TrackedEntryFilter::None => true,
            TrackedEntryFilter::Empty => false,
            TrackedEntryFilter::Nested if prefix.is_empty() => true,
            TrackedEntryFilter::Nested | TrackedEntryFilter::All => {
                self.path_filters.is_empty()
                    || self
                        .path_filters
                        .iter()
                        .any(|filter| path_matches_filter(&path, filter))
            }
        }
    }

    fn entry_pathspecs(&self, prefix: &str) -> Option<EntryPathspecs> {
        if self.entry_filter == TrackedEntryFilter::All {
            return EntryPathspecs::from_filters(&self.path_filters);
        }
        if self.entry_filter != TrackedEntryFilter::Nested
            || prefix.is_empty()
            || self.path_filters.is_empty()
        {
            return None;
        }
        let prefix_path = prefix.trim_end_matches('/');
        let mut paths = Vec::new();
        for filter in &self.path_filters {
            if filter == "." || path_matches_filter(prefix_path, filter) {
                return None;
            }
            if let Some(child_filter) = filter.strip_prefix(prefix)
                && !child_filter.is_empty()
            {
                paths.push(child_filter.to_owned());
            }
        }

        EntryPathspecs::from_filters(&paths)
    }
}

struct EntryPathspecs {
    paths: Vec<String>,
    gitlink_candidates: Vec<String>,
}

impl EntryPathspecs {
    fn from_filters(filters: &[String]) -> Option<Self> {
        if filters.is_empty() {
            return None;
        }
        let mut paths = Vec::new();
        let mut gitlink_candidates = Vec::new();
        for filter in filters {
            if filter.is_empty() {
                continue;
            }
            paths.push(filter.to_owned());
            if let Some((candidate, _)) = filter.split_once('/')
                && !gitlink_candidates
                    .iter()
                    .any(|existing| existing == candidate)
            {
                gitlink_candidates.push(candidate.to_owned());
            }
        }
        (!paths.is_empty()).then_some(Self {
            paths,
            gitlink_candidates,
        })
    }
}

fn record_tracked_entries_call(_root: &Path) {
    #[cfg(test)]
    let root = _root;
    #[cfg(test)]
    if let Some((observed_root, count)) = TRACKED_ENTRIES_OBSERVER
        .lock()
        .expect("tracked entries observer should lock")
        .as_mut()
        && observed_root == root
    {
        *count += 1;
    }
}

fn tracked_entries_inner(
    root: &Path,
    commit: &str,
    prefix: &str,
    depth: usize,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    record_tracked_entries_call(root);
    let root_key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let visit_key = (root_key, commit.to_owned());
    if !visited.insert(visit_key.clone()) {
        return Ok(GitTrackedEntries::default());
    }
    let bytes = match tracked_entries_ls_tree_bytes(root, commit, prefix, scope) {
        Ok(bytes) => bytes,
        Err(error) => {
            visited.remove(&visit_key);
            return Err(error);
        }
    };
    let mut state = GitTrackedEntries::default();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        match fields.get(1).copied() {
            Some("blob") if scope.allows_entry(prefix, path) => {
                push_blob_entry(prefix, path, &fields, &mut state.entries);
            }
            Some("commit")
                if depth < MAX_SUBMODULE_EXPANSION_DEPTH
                    && scope.allows_submodule_expansion(&format!("{prefix}{path}")) =>
            {
                let Some(submodule_commit) = fields.get(2) else {
                    continue;
                };
                let next_prefix = format!("{prefix}{path}/");
                match tracked_submodule_entries(
                    TrackedSubmoduleRequest {
                        root,
                        parent_commit: commit,
                        path,
                        submodule_commit,
                        prefix: &next_prefix,
                        depth: depth + 1,
                    },
                    visited,
                    scope,
                ) {
                    Ok(mut submodule_state) => {
                        state
                            .submodule_states
                            .push(format!("expanded\0{prefix}{path}\0{submodule_commit}"));
                        state.entries.append(&mut submodule_state.entries);
                        state
                            .submodule_states
                            .append(&mut submodule_state.submodule_states);
                    }
                    Err(_) => {
                        state
                            .submodule_states
                            .push(format!("unavailable\0{prefix}{path}\0{submodule_commit}"));
                    }
                }
            }
            _ => {}
        }
    }

    visited.remove(&visit_key);

    Ok(state)
}

struct TrackedSubmoduleRequest<'a> {
    root: &'a Path,
    parent_commit: &'a str,
    path: &'a str,
    submodule_commit: &'a str,
    prefix: &'a str,
    depth: usize,
}

fn tracked_submodule_entries(
    request: TrackedSubmoduleRequest<'_>,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    if let Ok(submodule_root) = submodule_worktree_root(request.root, request.path) {
        match tracked_entries_inner(
            &submodule_root,
            request.submodule_commit,
            request.prefix,
            request.depth,
            visited,
            scope,
        ) {
            Ok(state) => return Ok(state),
            Err(error) if tracked_entries_commit_lookup_failed(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let git_dir = submodule_git_dir(
        request.root,
        request.path,
        Some(request.parent_commit),
        Some(request.submodule_commit),
    )?;
    tracked_entries_from_git_dir_inner(
        &git_dir,
        request.submodule_commit,
        request.prefix,
        request.depth,
        visited,
        scope,
    )
}

fn tracked_entries_commit_lookup_failed(error: &CodeIndexError) -> bool {
    matches!(error, CodeIndexError::Git { args, .. } if args.iter().any(|arg| arg == "ls-tree"))
}

fn push_blob_entry(prefix: &str, path: &str, fields: &[&str], entries: &mut Vec<GitTreeEntry>) {
    let byte_count = fields
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    entries.push(GitTreeEntry {
        path: format!("{prefix}{path}"),
        byte_count,
    });
}

pub(super) fn submodule_worktree_root(root: &Path, path: &str) -> Result<PathBuf, CodeIndexError> {
    let worktree = root.join(path);
    if !fs::symlink_metadata(&worktree)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
    {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule worktree for path {path} is unavailable"
        )));
    }

    let resolved = resolve_git_root(&worktree)?;
    let worktree_root = worktree.canonicalize().unwrap_or(worktree);
    let resolved_root = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if resolved_root != worktree_root {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule worktree for path {path} resolves to parent repository"
        )));
    }

    Ok(resolved)
}

pub(super) fn tracked_entries_from_git_dir_with_scope(
    git_dir: &Path,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    let mut visited = BTreeSet::new();
    Ok(tracked_entries_from_git_dir_inner(git_dir, commit, "", 0, &mut visited, scope)?.entries)
}

fn tracked_entries_from_git_dir_inner(
    git_dir: &Path,
    commit: &str,
    prefix: &str,
    depth: usize,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let git_dir_key = git_dir
        .canonicalize()
        .unwrap_or_else(|_| git_dir.to_path_buf());
    let visit_key = (git_dir_key, commit.to_owned());
    if !visited.insert(visit_key.clone()) {
        return Ok(GitTrackedEntries::default());
    }
    let bytes = match tracked_entries_git_dir_ls_tree_bytes(git_dir, commit, prefix, scope) {
        Ok(bytes) => bytes,
        Err(error) => {
            visited.remove(&visit_key);
            return Err(error);
        }
    };
    let mut state = GitTrackedEntries::default();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        match fields.get(1).copied() {
            Some("blob") if scope.allows_entry(prefix, path) => {
                push_blob_entry(prefix, path, &fields, &mut state.entries);
            }
            Some("commit")
                if depth < MAX_SUBMODULE_EXPANSION_DEPTH
                    && scope.allows_submodule_expansion(&format!("{prefix}{path}")) =>
            {
                let Some(submodule_commit) = fields.get(2) else {
                    continue;
                };
                let next_prefix = format!("{prefix}{path}/");
                match tracked_git_dir_submodule_entries(
                    GitDirSubmoduleRequest {
                        parent_git_dir: git_dir,
                        parent_commit: commit,
                        path,
                        submodule_commit,
                        prefix: &next_prefix,
                        depth: depth + 1,
                    },
                    visited,
                    scope,
                ) {
                    Ok(mut submodule_state) => {
                        state
                            .submodule_states
                            .push(format!("expanded\0{prefix}{path}\0{submodule_commit}"));
                        state.entries.append(&mut submodule_state.entries);
                        state
                            .submodule_states
                            .append(&mut submodule_state.submodule_states);
                    }
                    Err(_) => {
                        state
                            .submodule_states
                            .push(format!("unavailable\0{prefix}{path}\0{submodule_commit}"));
                    }
                }
            }
            _ => {}
        }
    }

    visited.remove(&visit_key);

    Ok(state)
}

fn tracked_entries_ls_tree_bytes(
    root: &Path,
    commit: &str,
    prefix: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<u8>, CodeIndexError> {
    let Some(pathspecs) = scope.entry_pathspecs(prefix) else {
        return git_bytes(root, ["ls-tree", "-r", "-l", "-z", commit]);
    };
    let mut args = vec!["ls-tree", "-r", "-l", "-z", commit, "--"];
    args.extend(pathspecs.paths.iter().map(String::as_str));
    let mut bytes = git_bytes_slice(root, &args)?;
    for candidate in &pathspecs.gitlink_candidates {
        let mut candidate_bytes =
            git_bytes_slice(root, &["ls-tree", "-l", "-z", commit, "--", candidate])?;
        bytes.append(&mut candidate_bytes);
    }

    Ok(bytes)
}

fn tracked_entries_git_dir_ls_tree_bytes(
    git_dir: &Path,
    commit: &str,
    prefix: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<u8>, CodeIndexError> {
    let Some(pathspecs) = scope.entry_pathspecs(prefix) else {
        return git_dir_bytes(git_dir, &["ls-tree", "-r", "-l", "-z", commit]);
    };
    let mut args = vec!["ls-tree", "-r", "-l", "-z", commit, "--"];
    args.extend(pathspecs.paths.iter().map(String::as_str));
    let mut bytes = git_dir_bytes(git_dir, &args)?;
    for candidate in &pathspecs.gitlink_candidates {
        let mut candidate_bytes =
            git_dir_bytes(git_dir, &["ls-tree", "-l", "-z", commit, "--", candidate])?;
        bytes.append(&mut candidate_bytes);
    }

    Ok(bytes)
}

struct GitDirSubmoduleRequest<'a> {
    parent_git_dir: &'a Path,
    parent_commit: &'a str,
    path: &'a str,
    submodule_commit: &'a str,
    prefix: &'a str,
    depth: usize,
}

fn tracked_git_dir_submodule_entries(
    request: GitDirSubmoduleRequest<'_>,
    visited: &mut BTreeSet<(PathBuf, String)>,
    scope: &TrackedEntryScope,
) -> Result<GitTrackedEntries, CodeIndexError> {
    let git_dir = submodule_git_dir_from_git_dir(
        request.parent_git_dir,
        request.path,
        Some(request.parent_commit),
        Some(request.submodule_commit),
    )?;
    tracked_entries_from_git_dir_inner(
        &git_dir,
        request.submodule_commit,
        request.prefix,
        request.depth,
        visited,
        scope,
    )
}

pub(super) fn submodule_git_dir(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    submodule_commit: Option<&str>,
) -> Result<PathBuf, CodeIndexError> {
    for name in submodule_names_for_path(root, path, parent_commit) {
        if let Ok(git_dir) = submodule_git_dir_for_name(root, &name)
            && submodule_git_dir_matches_commit(&git_dir, submodule_commit)
        {
            return Ok(git_dir);
        }
    }
    if let Ok(git_dir) = submodule_git_dir_for_name(root, path.trim_matches('/'))
        && submodule_git_dir_matches_commit(&git_dir, submodule_commit)
    {
        return Ok(git_dir);
    }
    Err(CodeIndexError::InvalidInput(format!(
        "submodule git dir for path {path} is unavailable"
    )))
}

pub(super) fn submodule_git_dir_from_git_dir(
    git_dir: &Path,
    path: &str,
    parent_commit: Option<&str>,
    submodule_commit: Option<&str>,
) -> Result<PathBuf, CodeIndexError> {
    for name in submodule_names_for_path_from_git_dir(git_dir, path, parent_commit) {
        if let Ok(submodule_git_dir) = submodule_git_dir_for_name_from_git_dir(git_dir, &name)
            && submodule_git_dir_matches_commit(&submodule_git_dir, submodule_commit)
        {
            return Ok(submodule_git_dir);
        }
    }
    if let Ok(submodule_git_dir) =
        submodule_git_dir_for_name_from_git_dir(git_dir, path.trim_matches('/'))
        && submodule_git_dir_matches_commit(&submodule_git_dir, submodule_commit)
    {
        return Ok(submodule_git_dir);
    }
    Err(CodeIndexError::InvalidInput(format!(
        "nested submodule git dir for path {path} is unavailable"
    )))
}

pub(super) fn git_dir_bytes(git_dir: &Path, args: &[&str]) -> Result<Vec<u8>, CodeIndexError> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(git_dir)
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

fn submodule_names_for_path(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_submodule_names_from_config(
        git_bytes(root, ["config", "--get-regexp", "^submodule\\..*\\.path$"])
            .ok()
            .as_deref(),
        path,
        &mut names,
    );
    collect_submodule_names_from_config(
        git_bytes(
            root,
            [
                "config",
                "--file",
                ".gitmodules",
                "--get-regexp",
                "^submodule\\..*\\.path$",
            ],
        )
        .ok()
        .as_deref(),
        path,
        &mut names,
    );
    if let Some(parent_commit) = parent_commit {
        let object = format!("{parent_commit}:.gitmodules");
        collect_submodule_names_from_gitmodules(
            git_bytes(root, ["show", &object]).ok().as_deref(),
            path,
            &mut names,
        );
    }

    names
}

fn submodule_names_for_path_from_git_dir(
    git_dir: &Path,
    path: &str,
    parent_commit: Option<&str>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_submodule_names_from_config(
        git_dir_bytes(
            git_dir,
            &["config", "--get-regexp", "^submodule\\..*\\.path$"],
        )
        .ok()
        .as_deref(),
        path,
        &mut names,
    );
    if let Some(parent_commit) = parent_commit {
        let object = format!("{parent_commit}:.gitmodules");
        collect_submodule_names_from_gitmodules(
            git_dir_bytes(git_dir, &["show", &object]).ok().as_deref(),
            path,
            &mut names,
        );
    }

    names
}

fn collect_submodule_names_from_config(
    bytes: Option<&[u8]>,
    path: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(bytes) = bytes else {
        return;
    };
    for line in String::from_utf8_lossy(bytes).lines() {
        let Some((key, value)) = split_config_key_value(line) else {
            continue;
        };
        if value.trim() != path {
            continue;
        }
        let Some(name) = key
            .strip_prefix("submodule.")
            .and_then(|value| value.strip_suffix(".path"))
        else {
            continue;
        };
        names.insert(name.to_owned());
    }
}

fn collect_submodule_names_from_gitmodules(
    bytes: Option<&[u8]>,
    path: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(bytes) = bytes else {
        return;
    };
    let mut current_name = None::<String>;
    for raw_line in String::from_utf8_lossy(bytes).lines() {
        let line = raw_line.trim();
        if let Some(name) = gitmodules_section_name(line) {
            current_name = Some(name);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "path"
            && value.trim() == path
            && let Some(name) = &current_name
        {
            names.insert(name.clone());
        }
    }
}

fn split_config_key_value(line: &str) -> Option<(&str, &str)> {
    let split_at = line.find(char::is_whitespace)?;
    Some((&line[..split_at], line[split_at..].trim()))
}

fn gitmodules_section_name(line: &str) -> Option<String> {
    line.strip_prefix("[submodule \"")
        .and_then(|value| value.strip_suffix("\"]"))
        .map(ToOwned::to_owned)
}

fn submodule_git_dir_for_name(root: &Path, name: &str) -> Result<PathBuf, CodeIndexError> {
    validate_submodule_name(name)?;
    let git_path = format!("modules/{name}");
    let bytes = git_bytes(
        root,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &git_path,
        ],
    )?;
    let git_dir = PathBuf::from(String::from_utf8_lossy(&bytes).trim().to_owned());
    if git_dir.exists() {
        return Ok(git_dir);
    }

    Err(CodeIndexError::InvalidInput(format!(
        "submodule git dir for name {name} is unavailable"
    )))
}

fn submodule_git_dir_for_name_from_git_dir(
    git_dir: &Path,
    name: &str,
) -> Result<PathBuf, CodeIndexError> {
    validate_submodule_name(name)?;
    let git_path = format!("modules/{name}");
    let bytes = git_dir_bytes(
        git_dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &git_path,
        ],
    )?;
    let submodule_git_dir = PathBuf::from(String::from_utf8_lossy(&bytes).trim().to_owned());
    if submodule_git_dir.exists() {
        return Ok(submodule_git_dir);
    }

    Err(CodeIndexError::InvalidInput(format!(
        "nested submodule git dir for name {name} is unavailable"
    )))
}

fn validate_submodule_name(name: &str) -> Result<(), CodeIndexError> {
    if name.is_empty() {
        return Err(CodeIndexError::InvalidInput(
            "submodule name is empty".to_owned(),
        ));
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule name '{name}' cannot escape the repository git modules directory"
        )));
    }

    Ok(())
}

fn submodule_git_dir_matches_commit(git_dir: &Path, commit: Option<&str>) -> bool {
    commit.is_none_or(|commit| submodule_git_dir_has_commit(git_dir, commit))
}

fn submodule_git_dir_has_commit(git_dir: &Path, commit: &str) -> bool {
    if !git_dir.join("HEAD").exists() || !git_dir.join("objects").is_dir() {
        return false;
    }
    git_dir_bytes(
        git_dir,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
    )
    .is_ok()
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
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

fn path_matches_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }

    !filter.is_empty() && (path == filter || path.starts_with(&format!("{filter}/")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitTreeEntry {
    pub(super) path: String,
    pub(super) byte_count: usize,
}

pub(super) fn diff_changes(
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GitChange {
    AddedOrModified { path: String },
    Deleted { path: String },
    Renamed { old_path: String, new_path: String },
    Copied { old_path: String, new_path: String },
    TypeChanged { path: String },
}

pub(super) fn parse_name_status_z(bytes: &[u8]) -> Result<Vec<GitChange>, CodeIndexError> {
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

pub(super) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreePathChange {
    pub(super) status: String,
    pub(super) path: String,
    pub(super) deleted_source: Option<String>,
}

impl WorktreePathChange {
    pub(super) fn is_untracked(&self) -> bool {
        self.status == "??"
    }

    pub(super) fn has_index_change(&self) -> bool {
        self.status
            .as_bytes()
            .first()
            .is_some_and(|status| *status != b' ' && *status != b'?')
    }

    pub(super) fn has_worktree_change(&self) -> bool {
        self.status
            .as_bytes()
            .get(1)
            .is_some_and(|status| *status != b' ' && *status != b'?')
    }
}

pub(super) fn worktree_changed_paths(status: &[u8]) -> Vec<WorktreePathChange> {
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
