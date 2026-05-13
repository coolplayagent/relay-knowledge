use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::domain::{
    CodeImpactPathGroups, CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview,
    CodeRepositoryLargestFile, CodeRepositoryRegistration, CodeRepositoryScopePreview,
    CodeRepositorySelector,
};

use super::{
    CodeIndexError, git_bytes, git_object_exists, languages::language_id, resolve_ref,
    resolve_tree, split_nul,
};

const PREVIEW_MAX_EXCLUDED_PATHS: usize = 50;
const PREVIEW_MAX_LARGEST_FILES: usize = 10;
const DEFAULT_TEXT_FILE_BUDGET_BYTES: usize = 512 * 1024;
const DEFAULT_EXCLUDED_SEGMENTS: &[&str] = &[
    ".git",
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
const DEFAULT_EXCLUDED_EXTENSIONS: &[&str] = &[
    "7z", "avif", "bmp", "bz2", "class", "eot", "gif", "gz", "ico", "jar", "jpeg", "jpg", "lockb",
    "map", "mov", "mp4", "otf", "pdf", "png", "svg", "tar", "tgz", "ttf", "wasm", "webm", "woff",
    "woff2", "zip", "zst",
];
const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &[".relay-knowledgeignore"];

/// Returns a non-mutating preview of the effective repository indexing scope.
pub fn preview_repository_scope(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let commit = resolve_ref(&root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(&root, &commit)?;
    let ignore_rules = load_ignore_rules_from_commit(&root, &commit)?;
    let mut selected_byte_count = 0usize;
    let mut selected_file_count = 0usize;
    let mut unsupported_file_count = 0usize;
    let mut generated_or_heavy_file_count = 0usize;
    let mut expected_degraded_file_count = 0usize;
    let mut language_distribution = BTreeMap::<String, (usize, usize)>::new();
    let mut largest_files = Vec::<CodeRepositoryLargestFile>::new();
    let mut excluded_paths = Vec::<CodeRepositoryExcludedPath>::new();

    for entry in tracked_entries(&root, &commit)? {
        if let Some(reason) =
            selection_exclusion_reason(&entry.path, registration, selector, &ignore_rules)
        {
            if excluded_paths.len() < PREVIEW_MAX_EXCLUDED_PATHS {
                excluded_paths.push(CodeRepositoryExcludedPath {
                    path: entry.path,
                    reason,
                });
            }
            continue;
        }
        let language = language_id(&entry.path).unwrap_or("unknown");
        selected_file_count += 1;
        selected_byte_count = selected_byte_count.saturating_add(entry.byte_count);
        let bucket = language_distribution
            .entry(language.to_owned())
            .or_insert((0, 0));
        bucket.0 += 1;
        bucket.1 = bucket.1.saturating_add(entry.byte_count);
        let is_unsupported = language == "unknown";
        let is_heavy = entry.byte_count > DEFAULT_TEXT_FILE_BUDGET_BYTES;
        if is_unsupported {
            unsupported_file_count += 1;
        }
        if is_heavy {
            generated_or_heavy_file_count += 1;
        }
        if is_unsupported || is_heavy {
            expected_degraded_file_count += 1;
        }
        largest_files.push(CodeRepositoryLargestFile {
            path: entry.path,
            byte_count: entry.byte_count,
        });
    }
    largest_files.sort_by(|left, right| {
        right
            .byte_count
            .cmp(&left.byte_count)
            .then_with(|| left.path.cmp(&right.path))
    });
    largest_files.truncate(PREVIEW_MAX_LARGEST_FILES);

    Ok(CodeRepositoryScopePreview {
        repository_id: registration.repository_id.clone(),
        alias: registration.alias.clone(),
        requested_ref: selector.ref_selector.clone(),
        resolved_commit_sha: commit,
        tree_hash,
        selected_file_count,
        selected_byte_count,
        unsupported_file_count,
        generated_or_heavy_file_count,
        expected_degraded_file_count,
        language_distribution: language_distribution
            .into_iter()
            .map(
                |(language_id, (file_count, byte_count))| CodeRepositoryLanguagePreview {
                    language_id,
                    file_count,
                    byte_count,
                },
            )
            .collect(),
        largest_files,
        excluded_paths,
    })
}

/// Splits diff paths by the same selector rules used by indexing and impact.
pub fn partition_changed_paths_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    paths: Vec<String>,
) -> Result<CodeImpactPathGroups, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let commit = resolve_ref(&root, &selector.ref_selector)?;
    let ignore_rules = load_ignore_rules_from_commit(&root, &commit)?;
    let mut in_scope_changed_paths = Vec::new();
    let mut out_of_scope_changed_paths = Vec::new();
    for path in paths {
        if selection_exclusion_reason(&path, registration, selector, &ignore_rules).is_none() {
            in_scope_changed_paths.push(path);
        } else {
            out_of_scope_changed_paths.push(path);
        }
    }
    in_scope_changed_paths.sort();
    in_scope_changed_paths.dedup();
    out_of_scope_changed_paths.sort();
    out_of_scope_changed_paths.dedup();

    Ok(CodeImpactPathGroups {
        in_scope_changed_paths,
        out_of_scope_changed_paths,
    })
}

#[cfg(test)]
pub(super) fn path_is_selected(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    let root = Path::new(&registration.root_path);
    let ignore_rules = load_ignore_rules(root).expect("ignore rules should load in tests");

    path_is_selected_with_rules(path, registration, selector, &ignore_rules)
}

pub(super) fn path_is_selected_with_rules(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    ignore_rules: &[IgnoreRule],
) -> bool {
    selection_exclusion_reason(path, registration, selector, ignore_rules).is_none()
}

pub(super) fn selection_exclusion_reason(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    ignore_rules: &[IgnoreRule],
) -> Option<String> {
    if !path_scope_allows(path, registration, selector) {
        return Some("outside registered/requested path scope".to_owned());
    }
    if !language_filter_allows(path, &registration.language_filters)
        || !language_filter_allows(path, &selector.language_filters)
    {
        return Some("outside registered/requested language scope".to_owned());
    }
    if ignore_rules.iter().any(|rule| rule.matches(path)) {
        return Some("excluded by .relay-knowledgeignore".to_owned());
    }
    if default_source_preset_excludes(path)
        && !explicit_path_filter_opts_into_default_exclusion(
            path,
            registration
                .path_filters
                .iter()
                .chain(selector.path_filters.iter()),
        )
    {
        return Some("excluded by source preset".to_owned());
    }

    None
}

pub(super) fn path_scope_allows(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    path_filter_allows(path, &registration.path_filters)
        && path_filter_allows(path, &selector.path_filters)
}

pub(super) fn path_scope_overlaps(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    path_filter_overlaps(path, &registration.path_filters)
        && path_filter_overlaps(path, &selector.path_filters)
}

pub(super) fn load_ignore_rules(root: &Path) -> Result<Vec<IgnoreRule>, CodeIndexError> {
    let path = root.join(".relay-knowledgeignore");
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    Ok(parse_ignore_rules(&content))
}

pub(super) fn load_ignore_rules_from_commit(
    root: &Path,
    commit: &str,
) -> Result<Vec<IgnoreRule>, CodeIndexError> {
    let object = format!("{commit}:.relay-knowledgeignore");
    if !git_object_exists(root, &object)? {
        return Ok(Vec::new());
    }
    let content = String::from_utf8(git_bytes(root, ["show", &object])?).map_err(|error| {
        CodeIndexError::InvalidInput(format!(
            ".relay-knowledgeignore at {commit} is not valid UTF-8: {}",
            error.utf8_error()
        ))
    })?;

    Ok(parse_ignore_rules(&content))
}

fn parse_ignore_rules(content: &str) -> Vec<IgnoreRule> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('!'))
        .map(|line| IgnoreRule {
            pattern: line.trim_start_matches('/').to_owned(),
            anchored: line.starts_with('/'),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IgnoreRule {
    pattern: String,
    anchored: bool,
}

impl IgnoreRule {
    fn matches(&self, path: &str) -> bool {
        let pattern = normalize_path_filter(&self.pattern);
        let path = normalize_path_filter(path);
        if pattern.is_empty() {
            return false;
        }
        if let Some(extension) = pattern.strip_prefix("*.") {
            return if self.anchored {
                path.rsplit_once('/').is_none()
                    && path
                        .rsplit_once('.')
                        .is_some_and(|(_, path_extension)| path_extension == extension)
            } else {
                path.rsplit_once('.')
                    .is_some_and(|(_, path_extension)| path_extension == extension)
            };
        }
        if pattern.contains('/') {
            return path == pattern || path.starts_with(&format!("{pattern}/"));
        }
        if self.anchored {
            return path == pattern || path.starts_with(&format!("{pattern}/"));
        }
        path.split('/').any(|segment| segment == pattern)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitTreeEntry {
    path: String,
    byte_count: usize,
}

fn tracked_entries(root: &Path, commit: &str) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-r", "-l", "-z", commit])?;
    let mut entries = Vec::new();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        let byte_count = fields
            .get(3)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        entries.push(GitTreeEntry {
            path: path.to_owned(),
            byte_count,
        });
    }

    Ok(entries)
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

fn path_filter_overlaps(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_overlaps_filter(path, filter))
}

fn language_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || language_id(path)
            .map(|language| filters.iter().any(|filter| filter == language))
            .unwrap_or(false)
}

fn default_source_preset_excludes(path: &str) -> bool {
    let normalized = normalize_path_filter(path);
    if normalized
        .rsplit('/')
        .next()
        .is_some_and(|file_name| DEFAULT_EXCLUDED_FILENAMES.contains(&file_name))
    {
        return true;
    }
    if normalized
        .split('/')
        .any(|segment| DEFAULT_EXCLUDED_SEGMENTS.contains(&segment))
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

fn explicit_path_filter_opts_into_default_exclusion<'a>(
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
            DEFAULT_EXCLUDED_SEGMENTS.contains(segment)
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

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
