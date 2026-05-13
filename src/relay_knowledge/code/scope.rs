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
    CodeIndexError, git_bytes, languages::language_id, resolve_ref, resolve_tree, split_nul,
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
    let ignore_rules = load_ignore_rules(&root)?;
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
        if language == "unknown" {
            unsupported_file_count += 1;
            expected_degraded_file_count += 1;
        }
        if entry.byte_count > DEFAULT_TEXT_FILE_BUDGET_BYTES {
            generated_or_heavy_file_count += 1;
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
    let ignore_rules = load_ignore_rules(&root)?;
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

pub(super) fn path_is_selected(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    let root = Path::new(&registration.root_path);
    let ignore_rules = load_ignore_rules(root).unwrap_or_default();

    selection_exclusion_reason(path, registration, selector, &ignore_rules).is_none()
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

    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('!'))
        .map(|line| IgnoreRule {
            pattern: line.trim_start_matches('/').to_owned(),
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IgnoreRule {
    pattern: String,
}

impl IgnoreRule {
    fn matches(&self, path: &str) -> bool {
        let pattern = normalize_path_filter(&self.pattern);
        let path = normalize_path_filter(path);
        if pattern.is_empty() {
            return false;
        }
        if let Some(extension) = pattern.strip_prefix("*.") {
            return path
                .rsplit_once('.')
                .is_some_and(|(_, path_extension)| path_extension == extension);
        }
        if pattern.contains('/') {
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
    let path_segments = normalize_path_filter(path).split('/').collect::<Vec<_>>();
    filters.into_iter().any(|filter| {
        let filter = normalize_path_filter(filter);
        if filter.is_empty() || filter == "." {
            return false;
        }
        let filter_segments = filter.split('/').collect::<Vec<_>>();
        filter_segments.iter().any(|segment| {
            DEFAULT_EXCLUDED_SEGMENTS.contains(segment)
                || DEFAULT_EXCLUDED_EXTENSIONS
                    .contains(&segment.rsplit_once('.').map(|(_, ext)| ext).unwrap_or(""))
        }) || path_segments.starts_with(&filter_segments)
            && filter_segments
                .last()
                .is_some_and(|segment| DEFAULT_EXCLUDED_SEGMENTS.contains(segment))
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
