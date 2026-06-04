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
    changes::GitTreeEntry,
    languages::language_id,
    parser::dependency_manifest_language_ids,
    parser::dependency_manifest_overrides_default_exclusion,
    snapshot,
    source::{
        FileSystemScanPolicy, RepositorySourceKind, RepositorySourceSnapshot,
        filesystem_source_snapshot, source_snapshot,
    },
    source::{
        explicit_path_filter_opts_into_default_file_exclusion, filesystem_content_hashes_for_paths,
        filesystem_default_source_allows, filesystem_registration_identity,
        filesystem_tree_hash_from_path_hashes, source_commit_is_filesystem,
        source_default_file_preset_excludes, source_kind, source_language_filter_allows,
        source_path_has_indexable_content,
    },
    source_roots::{NESTED_SOURCE_MARKERS, STRIPPABLE_SOURCE_ROOTS},
};

const PREVIEW_MAX_EXCLUDED_PATHS: usize = 50;
const PREVIEW_MAX_LARGEST_FILES: usize = 10;
const DEFAULT_TEXT_FILE_BUDGET_BYTES: usize = 512 * 1024;
const SOURCE_LAYOUT_DISCOVERY_MAX_PATHS: usize = 200_000;
const SOURCE_LAYOUT_DISCOVERY_MAX_ROOTS: usize = 512;
const AUTO_SOURCE_SCOPE_FILTERS: &[&str] = &[".", "src", "include", "lib", "Sources"];

#[derive(Debug, Clone)]
pub(super) struct ScopedSourceSnapshot {
    pub(super) kind: RepositorySourceKind,
    pub(super) root: PathBuf,
    pub(super) resolved_commit_sha: String,
    pub(super) tree_hash: String,
    pub(super) entries: Vec<GitTreeEntry>,
    pub(super) content_hashes: BTreeMap<String, String>,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
}

pub(super) fn scoped_source_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &std::path::Path,
    ref_selector: &str,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let allow_filesystem_ref =
        registration_allows_filesystem_ref(registration, root, ref_selector)?;
    scoped_source_snapshot_inner(
        registration,
        selector,
        root,
        ref_selector,
        allow_filesystem_ref,
    )
}

pub(super) fn scoped_source_snapshot_for_filters(
    root: &std::path::Path,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let registration = CodeRepositoryRegistration {
        repository_id: "repo".to_owned(),
        alias: "alias".to_owned(),
        root_path: root.display().to_string(),
        path_filters: path_filters.to_vec(),
        language_filters: language_filters.to_vec(),
    };
    let selector = CodeRepositorySelector {
        repository: "alias".to_owned(),
        ref_selector: ref_selector.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };

    scoped_source_snapshot_inner(&registration, &selector, root, ref_selector, true)
}

pub(super) fn scoped_source_snapshot_for_registration(
    registration: &CodeRepositoryRegistration,
    ref_selector: &str,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let selector = CodeRepositorySelector {
        repository: registration.alias.clone(),
        ref_selector: ref_selector.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };

    scoped_source_snapshot(registration, &selector, &root, ref_selector)
}

pub(super) fn scoped_source_snapshot_for_registration_filters(
    registration: &CodeRepositoryRegistration,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let selector = CodeRepositorySelector {
        repository: registration.alias.clone(),
        ref_selector: ref_selector.to_owned(),
        path_filters: path_filters.to_vec(),
        language_filters: language_filters.to_vec(),
    };

    scoped_source_snapshot(registration, &selector, &root, ref_selector)
}

fn scoped_source_snapshot_inner(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &std::path::Path,
    ref_selector: &str,
    allow_filesystem_ref: bool,
) -> Result<ScopedSourceSnapshot, CodeIndexError> {
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let snapshot =
        source_snapshot_for_scope(root, ref_selector, filesystem_policy, allow_filesystem_ref)?;
    let source_layout = discover_source_layout(&snapshot.entries);
    let path_filters = effective_index_path_filters(registration, selector, &source_layout);
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| {
            selection_exclusion_reason_for_source(
                &entry.path,
                registration,
                selector,
                &source_layout,
                snapshot.kind,
            )
            .is_none()
        })
        .collect::<Vec<_>>();
    let (resolved_commit_sha, tree_hash, content_hashes) = if snapshot.kind.is_filesystem() {
        scoped_filesystem_tree_hash(&snapshot.root, &entries, ref_selector)?
    } else {
        (
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            BTreeMap::new(),
        )
    };

    Ok(ScopedSourceSnapshot {
        kind: snapshot.kind,
        root: snapshot.root,
        resolved_commit_sha,
        tree_hash,
        entries,
        content_hashes,
        path_filters,
        language_filters,
    })
}

fn source_snapshot_for_scope(
    root: &std::path::Path,
    ref_selector: &str,
    filesystem_policy: FileSystemScanPolicy,
    allow_filesystem_ref: bool,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(ref_selector) && allow_filesystem_ref {
        return filesystem_source_snapshot(root, filesystem_policy);
    }

    source_snapshot(root, ref_selector, filesystem_policy)
}

pub(super) fn filesystem_policy_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> FileSystemScanPolicy {
    let filters = intersect_path_filters(&registration.path_filters, &selector.path_filters);
    let policy = FileSystemScanPolicy::from_path_and_language_filters(
        filters.as_deref().unwrap_or(&[]),
        &registration.language_filters,
        &selector.language_filters,
    );
    if filters.is_none() {
        policy.with_denied_path_scope()
    } else {
        policy
    }
}

fn registration_allows_filesystem_ref(
    registration: &CodeRepositoryRegistration,
    root: &std::path::Path,
    ref_selector: &str,
) -> Result<bool, CodeIndexError> {
    if !source_commit_is_filesystem(ref_selector) {
        return Ok(false);
    }
    if registration.repository_id == filesystem_registration_identity(root)? {
        return Ok(true);
    }

    Ok(source_kind(root)?.is_filesystem())
}

pub(super) fn scoped_filesystem_tree_hash(
    root: &std::path::Path,
    entries: &[GitTreeEntry],
    ref_selector: &str,
) -> Result<(String, String, BTreeMap<String, String>), CodeIndexError> {
    let paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let content_hashes = filesystem_content_hashes_for_paths(root, &paths)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&content_hashes);
    if source_commit_is_filesystem(ref_selector) && ref_selector != tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {ref_selector} no longer matches live indexed scope {tree_hash}"
        )));
    }

    Ok((tree_hash.clone(), tree_hash, content_hashes))
}

/// Returns a non-mutating preview of the effective repository indexing scope.
pub fn preview_repository_scope(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let allow_filesystem_ref =
        registration_allows_filesystem_ref(registration, &root, &selector.ref_selector)?;
    let snapshot = source_snapshot_for_scope(
        &root,
        &selector.ref_selector,
        filesystem_policy,
        allow_filesystem_ref,
    )?;
    let mut selected_byte_count = 0usize;
    let mut selected_file_count = 0usize;
    let mut unsupported_file_count = 0usize;
    let mut generated_or_heavy_file_count = 0usize;
    let mut expected_degraded_file_count = 0usize;
    let mut language_distribution = BTreeMap::<String, (usize, usize)>::new();
    let mut largest_files = Vec::<CodeRepositoryLargestFile>::new();
    let mut excluded_paths = Vec::<CodeRepositoryExcludedPath>::new();

    let entries = snapshot.entries;
    let source_layout = discover_source_layout(&entries);
    let mut selected_entries = Vec::new();
    for entry in entries {
        if let Some(reason) = selection_exclusion_reason_for_source(
            &entry.path,
            registration,
            selector,
            &source_layout,
            snapshot.kind,
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
            path: entry.path.clone(),
            byte_count: entry.byte_count,
        });
        selected_entries.push(entry);
    }
    let (resolved_commit_sha, tree_hash, _) = if snapshot.kind.is_filesystem() {
        scoped_filesystem_tree_hash(&snapshot.root, &selected_entries, &selector.ref_selector)?
    } else {
        (
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            BTreeMap::new(),
        )
    };
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
        resolved_commit_sha,
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
    if paths.is_empty() {
        return Ok(CodeImpactPathGroups {
            in_scope_changed_paths: Vec::new(),
            out_of_scope_changed_paths: Vec::new(),
        });
    }
    let root = PathBuf::from(&registration.root_path);
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let (source_layout, source_kind) = if source_commit_is_filesystem(&selector.ref_selector) {
        let snapshot =
            scoped_source_snapshot(registration, selector, &root, &selector.ref_selector)?;
        (discover_source_layout(&snapshot.entries), snapshot.kind)
    } else {
        let snapshot = source_snapshot(&root, &selector.ref_selector, filesystem_policy)?;
        (discover_source_layout(&snapshot.entries), snapshot.kind)
    };
    let mut in_scope_changed_paths = Vec::new();
    let mut out_of_scope_changed_paths = Vec::new();
    for path in paths {
        if selection_exclusion_reason_for_source(
            &path,
            registration,
            selector,
            &source_layout,
            source_kind,
        )
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
    selection_exclusion_reason_for_source(
        path,
        registration,
        selector,
        source_layout,
        RepositorySourceKind::Git,
    )
}

pub(super) fn selection_exclusion_reason_for_source(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layout: &SourceLayoutDiscovery,
    source_kind: RepositorySourceKind,
) -> Option<String> {
    if !path_scope_allows(path, registration, selector)
        && !source_layout.extends_path_scope(path, registration, selector)
    {
        return Some("outside registered/requested path scope".to_owned());
    }
    if source_kind.is_filesystem()
        && filesystem_default_scope_excludes(path, registration, selector)
    {
        return Some("outside non-git default source whitelist".to_owned());
    }
    if !source_language_filter_allows(path, &registration.language_filters)
        || !source_language_filter_allows(path, &selector.language_filters)
    {
        return Some("outside registered/requested language scope".to_owned());
    }
    if source_default_file_preset_excludes(path)
        && !dependency_manifest_overrides_default_exclusion(path)
        && !source_layout.keeps_default_excluded_source(path)
        && !explicit_path_filter_opts_into_default_file_exclusion(
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
            || source_default_file_preset_excludes(&entry.path)
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
    effective_index_path_filters_for_layouts(registration, selector, &[source_layout])
}

pub(super) fn effective_index_path_filters_for_layouts(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    source_layouts: &[&SourceLayoutDiscovery],
) -> Vec<String> {
    let mut filters = merged_path_filters(&registration.path_filters, &selector.path_filters);
    if !registration_scope_can_discover_source_roots(&registration.path_filters) {
        return filters;
    }
    for source_layout in source_layouts {
        for root in &source_layout.source_roots {
            if !selector_filter_allows_root(root, &selector.path_filters) {
                continue;
            }
            push_filter_if_uncovered(&mut filters, root);
        }
    }

    filters
}

fn filesystem_default_scope_excludes(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    if !registration.path_filters.is_empty() || !selector.path_filters.is_empty() {
        return false;
    }

    !filesystem_default_source_allows(path)
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

pub(super) fn intersect_path_filters(left: &[String], right: &[String]) -> Option<Vec<String>> {
    let left = normalized_path_filters(left);
    let right = normalized_path_filters(right);
    if left.is_empty() {
        return Some(right);
    }
    if right.is_empty() {
        return Some(left);
    }

    let mut intersections = Vec::new();
    for left_filter in &left {
        for right_filter in &right {
            if path_filter_covers(left_filter, right_filter) {
                push_filter_if_missing(&mut intersections, right_filter);
            } else if path_filter_covers(right_filter, left_filter) {
                push_filter_if_missing(&mut intersections, left_filter);
            }
        }
    }

    (!intersections.is_empty()).then_some(intersections)
}

pub(super) fn submodule_child_scope_filters(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Option<Vec<String>> {
    let filters = intersect_path_filters(&registration.path_filters, &selector.path_filters)?;
    if filters.is_empty() {
        return Some(Vec::new());
    }
    let path = normalize_scope_path(path);
    if path.is_empty() {
        return None;
    }
    let child_prefix = format!("{path}/");
    let mut child_filters = Vec::new();
    let mut parent_scope_covers_submodule = false;
    for filter in filters {
        let filter = normalize_scope_path(&filter);
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

fn normalize_scope_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_owned()
}

fn normalized_path_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|filter| normalize_path_filter(filter).to_owned())
        .filter(|filter| !filter.is_empty())
        .collect()
}

fn push_filter_if_missing(filters: &mut Vec<String>, filter: &str) {
    if !filters.iter().any(|existing| existing == filter) {
        filters.push(filter.to_owned());
    }
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
    if path_matches_filter(path, "include") {
        push_source_root(&mut roots, "include".to_owned());
    }
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
            assert!(!source_default_file_preset_excludes(path), "{path}");
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
        assert!(source_default_file_preset_excludes("uv.lock"));
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
