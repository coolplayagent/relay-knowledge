use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::domain::{
    CodeImpactPathGroups, CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview,
    CodeRepositoryLargestFile, CodeRepositoryRegistration, CodeRepositoryScopePreview,
    CodeRepositorySelector,
};

use super::{
    CodeIndexError,
    changes::{GitTreeEntry, tracked_entries},
    languages::language_id,
    parser::dependency_manifest_language_ids,
    parser::dependency_manifest_overrides_default_exclusion,
    resolve_ref, resolve_tree,
    source_roots::{NESTED_SOURCE_MARKERS, STRIPPABLE_SOURCE_ROOTS},
};

const PREVIEW_MAX_EXCLUDED_PATHS: usize = 50;
const PREVIEW_MAX_LARGEST_FILES: usize = 10;
const DEFAULT_TEXT_FILE_BUDGET_BYTES: usize = 512 * 1024;
const DEFAULT_EXCLUDED_EXTENSIONS: &[&str] = &[
    "7z", "avif", "bmp", "bz2", "class", "eot", "gif", "gz", "ico", "jar", "jpeg", "jpg", "jsonl",
    "lockb", "map", "mov", "mp4", "otf", "pdf", "png", "svg", "tar", "tgz", "ttf", "wasm", "webm",
    "woff", "woff2", "zip", "zst",
];
const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &["uv.lock"];
const SOURCE_LAYOUT_DISCOVERY_MAX_PATHS: usize = 200_000;
const SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS: usize = 512;
const AUTO_SOURCE_SCOPE_FILTERS: &[&str] = &[".", "src", "include", "lib", "Sources"];

/// Returns a non-mutating preview of the effective repository indexing scope.
pub fn preview_repository_scope(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let commit = resolve_ref(&root, &selector.ref_selector)?;
    let tree_hash = resolve_tree(&root, &commit)?;
    let mut selected_byte_count = 0usize;
    let mut selected_file_count = 0usize;
    let mut unsupported_file_count = 0usize;
    let mut generated_or_heavy_file_count = 0usize;
    let mut expected_degraded_file_count = 0usize;
    let mut language_distribution = BTreeMap::<String, (usize, usize)>::new();
    let mut largest_files = Vec::<CodeRepositoryLargestFile>::new();
    let mut excluded_paths = Vec::<CodeRepositoryExcludedPath>::new();

    let entries = tracked_entries(&root, &commit)?;
    let source_layout = discover_source_layout(&entries);
    for entry in entries {
        if let Some(reason) = selection_exclusion_reason_with_layout(
            &entry.path,
            registration,
            selector,
            &source_layout,
        ) {
            if excluded_paths.len() < PREVIEW_MAX_EXCLUDED_PATHS {
                excluded_paths.push(CodeRepositoryExcludedPath {
                    path: entry.path,
                    reason,
                });
            }
            continue;
        }
        let language = preview_language_id(&entry.path);
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
    let entries = tracked_entries(&root, &commit)?;
    let source_layout = discover_source_layout(&entries);
    let mut in_scope_changed_paths = Vec::new();
    let mut out_of_scope_changed_paths = Vec::new();
    for path in paths {
        if selection_exclusion_reason_with_layout(&path, registration, selector, &source_layout)
            .is_none()
        {
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
    selection_exclusion_reason(path, registration, selector).is_none()
}

pub(super) fn path_is_selected_with_layout(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> bool {
    selection_exclusion_reason_with_layout(path, registration, selector, source_layout).is_none()
}

pub(super) fn selection_exclusion_reason(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Option<String> {
    selection_exclusion_reason_with_layout(
        path,
        registration,
        selector,
        &SourceLayoutDiscovery::default(),
    )
}

pub(super) fn selection_exclusion_reason_with_layout(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> Option<String> {
    if !path_scope_allows(path, registration, selector)
        && !source_layout.extends_path_scope(path, registration, selector)
    {
        return Some("outside registered/requested path scope".to_owned());
    }
    if !language_filter_allows(path, &registration.language_filters)
        || !language_filter_allows(path, &selector.language_filters)
    {
        return Some("outside registered/requested language scope".to_owned());
    }
    if default_source_preset_excludes(path)
        && !dependency_manifest_overrides_default_exclusion(path)
        && !source_layout.keeps_default_excluded_source(path)
        && !explicit_path_filter_opts_into_default_exclusion(
            path,
            registration
                .path_filters
                .iter()
                .chain(selector.path_filters.iter()),
        )
    {
        return Some("excluded by file preset".to_owned());
    }

    None
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SourceLayoutDiscovery {
    source_roots: BTreeSet<String>,
}

impl SourceLayoutDiscovery {
    fn keeps_default_excluded_source(&self, path: &str) -> bool {
        source_path_has_indexable_content(path)
            && !path_contains_broad_dependency_segment(path)
            && self
                .source_roots
                .iter()
                .any(|root| path_matches_filter(path, root))
    }

    fn extends_path_scope(
        &self,
        path: &str,
        registration: &CodeRepositoryRegistration,
        selector: &CodeRepositorySelector,
    ) -> bool {
        registration_scope_can_discover_source_roots(&registration.path_filters)
            && selector_path_scope_allows_discovered_root(path, &selector.path_filters)
            && self.keeps_default_excluded_source(path)
    }
}

pub(super) fn discover_source_layout(entries: &[GitTreeEntry]) -> SourceLayoutDiscovery {
    let mut source_roots = BTreeSet::new();
    for entry in entries.iter().take(SOURCE_LAYOUT_DISCOVERY_MAX_PATHS) {
        if !source_path_has_indexable_content(&entry.path)
            || path_contains_broad_dependency_segment(&entry.path)
            || default_source_preset_excludes(&entry.path)
        {
            continue;
        }
        for root in source_layout_roots_for_path(&entry.path) {
            source_roots.insert(root);
            if source_roots.len() >= SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS {
                return SourceLayoutDiscovery { source_roots };
            }
        }
    }

    SourceLayoutDiscovery { source_roots }
}

pub(super) fn effective_index_path_filters(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
) -> Vec<String> {
    let mut filters = merged_path_filters(&registration.path_filters, &selector.path_filters);
    if !registration_scope_can_discover_source_roots(&registration.path_filters) {
        return filters;
    }
    for root in &source_layout.source_roots {
        if !selector_filter_allows_root(root, &selector.path_filters) {
            continue;
        }
        push_filter_if_uncovered(&mut filters, root);
    }

    filters
}

fn source_path_has_indexable_content(path: &str) -> bool {
    language_id(path).is_some() || dependency_manifest_language_ids(path).is_some()
}

fn preview_language_id(path: &str) -> &'static str {
    language_id(path).unwrap_or_else(|| {
        dependency_manifest_language_ids(path)
            .and_then(|languages| languages.first().copied())
            .unwrap_or("unknown")
    })
}

fn path_contains_broad_dependency_segment(path: &str) -> bool {
    normalize_path_filter(path)
        .split('/')
        .any(|segment| matches!(segment, "vendor" | "third_party" | "node_modules"))
}

fn registration_scope_can_discover_source_roots(filters: &[String]) -> bool {
    !filters.is_empty()
        && filters.iter().all(|filter| {
            let filter = normalize_path_filter(filter);
            AUTO_SOURCE_SCOPE_FILTERS.contains(&filter)
        })
}

fn selector_path_scope_allows_discovered_root(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(path, filter))
}

fn selector_filter_allows_root(root: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| path_matches_filter(root, filter) || path_overlaps_filter(root, filter))
}

fn merged_path_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for filter in left.iter().chain(right.iter()) {
        let normalized = normalize_path_filter(filter);
        if !normalized.is_empty() && !merged.iter().any(|existing| existing == normalized) {
            merged.push(normalized.to_owned());
        }
    }

    merged
}

fn push_filter_if_uncovered(filters: &mut Vec<String>, root: &str) {
    if filters
        .iter()
        .any(|filter| path_filter_covers(filter, root))
    {
        return;
    }
    filters.retain(|filter| !path_filter_covers(root, filter));
    filters.push(root.to_owned());
}

fn path_filter_covers(filter: &str, path: &str) -> bool {
    let filter = normalize_path_filter(filter);
    filter == "." || path_matches_filter(path, filter)
}

fn source_layout_roots_for_path(path: &str) -> Vec<String> {
    let path = normalize_path_filter(path);
    let mut roots = Vec::new();
    for marker in NESTED_SOURCE_MARKERS {
        if let Some((prefix, _)) = path.split_once(marker) {
            push_source_root(&mut roots, format!("{prefix}{marker}"));
        }
    }
    for root in STRIPPABLE_SOURCE_ROOTS {
        if let Some(suffix) = path.strip_prefix(root) {
            let mut segments = suffix.split('/').filter(|segment| !segment.is_empty());
            if let Some(first) = segments.next() {
                push_source_root(&mut roots, format!("{root}{first}"));
            } else {
                push_source_root(&mut roots, root.trim_end_matches('/').to_owned());
            }
        }
    }
    roots
}

fn push_source_root(roots: &mut Vec<String>, root: String) {
    let root = root.trim_end_matches('/').to_owned();
    if !root.is_empty() && !roots.contains(&root) {
        roots.push(root);
    }
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
    if filters.is_empty() {
        return true;
    }
    if language_id(path).is_some_and(|language| filters.iter().any(|filter| filter == language)) {
        return true;
    }
    dependency_manifest_language_ids(path).is_some_and(|languages| {
        languages
            .iter()
            .any(|language| filters.iter().any(|filter| filter == language))
    })
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

#[cfg(test)]
mod tests {
    use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

    use super::*;

    #[test]
    fn source_preset_does_not_exclude_tracked_directory_names() {
        for path in [
            "build/workflow.yaml",
            ".cloudbuild/cloudbuild.yaml",
            ".cid/pipeline.yml",
            ".build_config/settings.toml",
            "dist/bundle.js",
            "frontend/dist/js/components/sidebar.js",
            "node_modules/pkg/dist/js/core/index.js",
            "target/generated.rs",
            "vendor/pkg/lib.rs",
            "third_party/pkg/lib.rs",
        ] {
            assert!(!default_source_preset_excludes(path), "{path}");
        }
    }

    #[test]
    fn explicit_default_exclusion_opt_in_normalizes_extension_case() {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec!["assets/logo.SVG".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate");
        let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");

        assert!(path_is_selected(
            "assets/logo.SVG",
            &registration,
            &selector
        ));
    }

    #[test]
    fn default_file_preset_excludes_dataset_dumps_and_keeps_uv_lock_facts() {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![".".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate");
        let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");

        assert!(!path_is_selected(
            ".agent_teams/evals/datasets/swebench-verified-full.jsonl",
            &registration,
            &selector
        ));
        assert!(default_source_preset_excludes("uv.lock"));
        assert!(path_is_selected("uv.lock", &registration, &selector));
    }

    #[test]
    fn git_tracked_directory_names_are_selected_without_opt_in() {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![".".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate");
        let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");

        for path in [
            "build/workflow.yaml",
            ".cloudbuild/cloudbuild.yaml",
            ".cid/pipeline.yml",
            ".build_config/settings.toml",
            "external_deps/python_sdk/session_client.py",
            "packages/ui/src/index.ts",
            "modules/java_sdk/src/main/java/example/SessionClient.java",
            "plugins/example.com/nonstandard/session/client.go",
            "Sources/SwiftSdk/SessionClient.swift",
            "lib/app/controller.rb",
            "vendor/pkg/session_client.py",
            "third_party/pkg/session_client.py",
        ] {
            assert!(path_is_selected(path, &registration, &selector), "{path}");
        }
    }

    #[test]
    fn default_source_preset_keeps_file_extension_opt_in_scoped() {
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![".".to_owned(), "manual.pdf".to_owned()],
            Vec::new(),
        )
        .expect("registration should validate");
        let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");

        assert!(path_is_selected("manual.pdf", &registration, &selector));
        assert!(!path_is_selected("other.pdf", &registration, &selector));
    }
}
