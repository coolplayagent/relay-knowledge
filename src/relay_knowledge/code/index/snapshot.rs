use std::{collections::BTreeMap, path::Path};

use crate::domain::{
    CodeCallRecord, CodeIndexSnapshot, CodeMonorepoWorkspace, CodePathTombstone,
    CodeRepositoryRegistration, CodeRepositorySelector, CodeRouteRecord,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord, code_snapshot_scope_id,
};

use super::{identity, ids::stable_id};
use crate::code::{
    parser::workspace::WorkspaceSource,
    source::{RepositorySourceKind, changes::GitTreeEntry, source_snapshot_bytes},
};

struct IndexedWorkspaceSource<'a> {
    root_path: &'a Path,
    kind: RepositorySourceKind,
    commit: &'a str,
    entries: &'a [GitTreeEntry],
    path_filters: &'a [String],
}

impl WorkspaceSource for IndexedWorkspaceSource<'_> {
    fn root_path(&self) -> &Path {
        self.root_path
    }

    fn read_to_string(&self, relative_path: &str) -> Option<String> {
        let relative_path = indexed_workspace_path(relative_path)?;
        if !self.entries.iter().any(|entry| entry.path == relative_path)
            && !workspace_manifest_read_allowed(&relative_path, self.path_filters)
        {
            return None;
        }
        source_snapshot_bytes(self.root_path, self.kind, self.commit, &relative_path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    fn child_dirs(&self, relative_dir: &str) -> Vec<String> {
        let Some(parent) = indexed_workspace_path(relative_dir) else {
            return Vec::new();
        };
        let prefix = (!parent.is_empty()).then(|| format!("{parent}/"));
        let mut dirs = std::collections::BTreeSet::new();
        for entry in self.entries {
            let rest = match &prefix {
                Some(prefix) => entry.path.strip_prefix(prefix.as_str()),
                None => Some(entry.path.as_str()),
            };
            let Some(rest) = rest else {
                continue;
            };
            let Some((child, _)) = rest.split_once('/') else {
                continue;
            };
            if child.is_empty() {
                continue;
            }
            dirs.insert(match &prefix {
                Some(_) => format!("{parent}/{child}"),
                None => child.to_owned(),
            });
        }
        dirs.into_iter().collect()
    }

    fn descendant_dirs_containing_file(
        &self,
        relative_dir: &str,
        file_name: &str,
        directory_limit: usize,
        entry_limit: usize,
    ) -> Vec<String> {
        let Some(parent) = indexed_workspace_path(relative_dir) else {
            return Vec::new();
        };
        if directory_limit == 0
            || entry_limit == 0
            || file_name.trim().is_empty()
            || file_name.contains('/')
        {
            return Vec::new();
        }

        let prefix = (!parent.is_empty()).then(|| format!("{parent}/"));
        let mut dirs = std::collections::BTreeSet::new();
        for entry in self.entries.iter().take(entry_limit) {
            let rest = match &prefix {
                Some(prefix) => entry.path.strip_prefix(prefix.as_str()),
                None => Some(entry.path.as_str()),
            };
            let Some(rest) = rest else {
                continue;
            };
            let Some(dir) = rest
                .strip_suffix(file_name)
                .and_then(|value| value.strip_suffix('/'))
            else {
                continue;
            };
            if dir.is_empty() {
                continue;
            }

            let relative_path = match &prefix {
                Some(_) => format!("{parent}/{dir}"),
                None => dir.to_owned(),
            };
            dirs.insert(relative_path);
            if dirs.len() >= directory_limit {
                break;
            }
        }

        dirs.into_iter().collect()
    }
}

fn indexed_workspace_path(path: &str) -> Option<String> {
    let normalized = path.trim().replace('\\', "/");
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return None;
        }
        segments.push(segment);
    }
    Some(segments.join("/"))
}

pub(in crate::code) fn detect_workspaces_for_source_snapshot(
    root_path: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    entries: &[GitTreeEntry],
    path_filters: &[String],
    config: &crate::domain::CodeWorkspaceDetectionConfig,
) -> Vec<CodeMonorepoWorkspace> {
    let source = IndexedWorkspaceSource {
        root_path,
        kind,
        commit,
        entries,
        path_filters,
    };
    crate::code::parser::workspace::detect_workspaces_from_source(&source, config)
}

fn workspace_manifest_read_allowed(relative_path: &str, path_filters: &[String]) -> bool {
    is_workspace_manifest_path(relative_path) && workspace_path_allowed(relative_path, path_filters)
}

fn is_workspace_manifest_path(relative_path: &str) -> bool {
    matches!(
        relative_path.rsplit('/').next(),
        Some("pnpm-workspace.yaml" | "go.work" | "package.json" | "Cargo.toml" | "go.mod")
    )
}

fn workspace_path_allowed(relative_path: &str, path_filters: &[String]) -> bool {
    path_filters.is_empty()
        || path_filters.iter().any(|filter| {
            indexed_workspace_path(filter)
                .as_deref()
                .is_some_and(|filter| path_filter_covers(filter, relative_path))
        })
}

fn path_filter_covers(filter: &str, path: &str) -> bool {
    filter.is_empty()
        || path == filter
        || path
            .strip_prefix(filter)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub(in crate::code) struct SnapshotBuild {
    pub(in crate::code) repository_id: String,
    pub(in crate::code) source_scope: String,
    pub(in crate::code) base_resolved_commit_sha: Option<String>,
    pub(in crate::code) commit: String,
    pub(in crate::code) tree_hash: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    full_replace: bool,
    changed_path_count: usize,
    pub(in crate::code) skipped_unchanged_count: usize,
    pub(in crate::code) deleted_paths: Vec<String>,
    pub(in crate::code) tombstones: Vec<CodePathTombstone>,
    pub(in crate::code) files: Vec<crate::domain::RepositoryCodeFileRecord>,
    pub(in crate::code) symbols: Vec<RepositoryCodeSymbolRecord>,
    pub(in crate::code) references: Vec<RepositoryCodeReferenceRecord>,
    pub(in crate::code) imports: Vec<crate::domain::CodeImportRecord>,
    calls: Vec<CodeCallRecord>,
    pub(in crate::code) dependencies: Vec<crate::domain::CodeDependencyRecord>,
    pub(in crate::code) feature_flags: Vec<crate::domain::CodeFeatureFlagRecord>,
    pub(in crate::code) chunks: Vec<crate::domain::RepositoryCodeChunkRecord>,
    pub(in crate::code) routes: Vec<CodeRouteRecord>,
    pub(in crate::code) diagnostics: Vec<crate::domain::CodeFileDiagnostic>,
    /// Detected monorepo workspace members populated when
    /// [`CodeWorkspaceDetectionConfig::enabled`] is `true`.
    pub(in crate::code) workspaces: Vec<CodeMonorepoWorkspace>,
}

pub(in crate::code) struct SnapshotScopeFilters {
    pub(in crate::code) path_filters: Vec<String>,
    pub(in crate::code) language_filters: Vec<String>,
}

impl SnapshotBuild {
    #[cfg(test)]
    pub(in crate::code) fn new(
        registration: &CodeRepositoryRegistration,
        commit: String,
        tree_hash: String,
        full_replace: bool,
        changed_path_count: usize,
        skipped_unchanged_count: usize,
    ) -> Self {
        let selector = CodeRepositorySelector {
            repository: registration.repository_id.clone(),
            ref_selector: commit.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        };
        Self::new_with_selector(
            registration,
            &selector,
            commit,
            tree_hash,
            full_replace,
            changed_path_count,
            skipped_unchanged_count,
        )
    }

    pub(in crate::code) fn new_with_selector(
        registration: &CodeRepositoryRegistration,
        selector: &CodeRepositorySelector,
        commit: String,
        tree_hash: String,
        full_replace: bool,
        changed_path_count: usize,
        skipped_unchanged_count: usize,
    ) -> Self {
        let path_filters = merged_filters(&registration.path_filters, &selector.path_filters);
        let language_filters =
            merged_filters(&registration.language_filters, &selector.language_filters);
        Self::new_with_scope_filters(
            registration,
            commit,
            tree_hash,
            SnapshotScopeFilters {
                path_filters,
                language_filters,
            },
            full_replace,
            changed_path_count,
            skipped_unchanged_count,
        )
    }

    pub(in crate::code) fn new_with_scope_filters(
        registration: &CodeRepositoryRegistration,
        commit: String,
        tree_hash: String,
        filters: SnapshotScopeFilters,
        full_replace: bool,
        changed_path_count: usize,
        skipped_unchanged_count: usize,
    ) -> Self {
        let path_filters = filters.path_filters;
        let language_filters = filters.language_filters;
        let source_scope = code_snapshot_scope_id(
            &registration.repository_id,
            &tree_hash,
            &path_filters,
            &language_filters,
        );
        Self {
            repository_id: registration.repository_id.clone(),
            source_scope,
            base_resolved_commit_sha: None,
            commit,
            tree_hash,
            path_filters,
            language_filters,
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
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            routes: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
            workspaces: Vec::new(),
        }
    }

    pub(in crate::code) fn finish(mut self) -> CodeIndexSnapshot {
        identity::enrich_symbol_identities(&self.repository_id, &mut self.symbols);
        identity::resolve_reference_targets(&self.symbols, &mut self.references);
        identity::resolve_import_targets(&self.files, &self.symbols, &mut self.imports);
        let symbols_by_path = build_symbol_path_index(&self.symbols);
        let symbols_by_id = build_symbol_id_index(&self.symbols);
        self.calls = self
            .references
            .iter()
            .filter(|reference| reference.kind == "call")
            .map(|reference| {
                let caller = caller_for_line(
                    &symbols_by_path,
                    &reference.path,
                    reference.line_range.start,
                );
                let (caller_symbol_snapshot_id, caller_name) = caller
                    .map(|symbol| {
                        (
                            Some(symbol.symbol_snapshot_id.clone()),
                            Some(symbol.name.clone()),
                        )
                    })
                    .unwrap_or((None, None));

                let callee_name = reference
                    .target_symbol_snapshot_id
                    .as_deref()
                    .and_then(|symbol_id| symbols_by_id.get(symbol_id))
                    .map(|symbol| symbol.name.clone())
                    .or_else(|| reference.target_hint.clone())
                    .unwrap_or_else(|| reference.name.clone());

                CodeCallRecord {
                    repository_id: reference.repository_id.clone(),
                    source_scope: reference.source_scope.clone(),
                    call_id: stable_id(
                        "call",
                        [
                            self.repository_id.as_str(),
                            self.source_scope.as_str(),
                            reference.reference_id.as_str(),
                            reference.path.as_str(),
                            reference.name.as_str(),
                            &reference.line_range.start.to_string(),
                        ],
                    ),
                    file_id: reference.file_id.clone(),
                    path: reference.path.clone(),
                    caller_symbol_snapshot_id,
                    caller_name,
                    callee_symbol_snapshot_id: reference.target_symbol_snapshot_id.clone(),
                    callee_name,
                    target_hint: reference.target_hint.clone(),
                    resolution_state: reference.resolution_state.clone(),
                    confidence_basis_points: reference.confidence_basis_points,
                    confidence_tier: reference.confidence_tier.clone(),
                    line_range: reference.line_range.clone(),
                }
            })
            .collect();

        CodeIndexSnapshot {
            repository_id: self.repository_id,
            source_scope: self.source_scope,
            base_resolved_commit_sha: self.base_resolved_commit_sha,
            resolved_commit_sha: self.commit,
            tree_hash: self.tree_hash,
            path_filters: self.path_filters,
            language_filters: self.language_filters,
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
            dependencies: self.dependencies,
            feature_flags: self.feature_flags,
            routes: self.routes,
            chunks: self.chunks,
            workspaces: self.workspaces,
            diagnostics: self.diagnostics,
        }
    }

    pub(in crate::code) fn append_file_records(&mut self, mut other: Self) {
        debug_assert_eq!(self.repository_id, other.repository_id);
        debug_assert_eq!(self.source_scope, other.source_scope);
        self.files.append(&mut other.files);
        self.symbols.append(&mut other.symbols);
        self.references.append(&mut other.references);
        self.imports.append(&mut other.imports);
        self.calls.append(&mut other.calls);
        self.dependencies.append(&mut other.dependencies);
        self.feature_flags.append(&mut other.feature_flags);
        self.routes.append(&mut other.routes);
        self.chunks.append(&mut other.chunks);
        self.diagnostics.append(&mut other.diagnostics);
        self.workspaces.append(&mut other.workspaces);
    }

    pub(in crate::code) fn language_filters(&self) -> &[String] {
        &self.language_filters
    }

    /// Scans the repository root for monorepo workspace manifests and
    /// populates `self.workspaces` when detection is enabled.
    ///
    /// This is a no-op when [`CodeWorkspaceDetectionConfig::enabled`] is
    /// `false` or when no supported workspace manifests are found.
    pub(in crate::code) fn detect_and_fill_workspaces(
        &mut self,
        root_path: &Path,
        kind: RepositorySourceKind,
        entries: &[GitTreeEntry],
        config: &crate::domain::CodeWorkspaceDetectionConfig,
    ) {
        let commit = self.commit.clone();
        self.detect_and_fill_workspaces_at_commit(root_path, kind, &commit, entries, config);
    }

    pub(in crate::code) fn detect_and_fill_workspaces_at_commit(
        &mut self,
        root_path: &Path,
        kind: RepositorySourceKind,
        source_commit: &str,
        entries: &[GitTreeEntry],
        config: &crate::domain::CodeWorkspaceDetectionConfig,
    ) {
        self.workspaces = detect_workspaces_for_source_snapshot(
            root_path,
            kind,
            source_commit,
            entries,
            &self.path_filters,
            config,
        );
    }
}

fn build_symbol_path_index(
    symbols: &[RepositoryCodeSymbolRecord],
) -> BTreeMap<&str, Vec<&RepositoryCodeSymbolRecord>> {
    let mut index = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        index.entry(&symbol.path).or_default().push(symbol);
    }

    index
}

fn build_symbol_id_index(
    symbols: &[RepositoryCodeSymbolRecord],
) -> BTreeMap<&str, &RepositoryCodeSymbolRecord> {
    let mut index = BTreeMap::<&str, &RepositoryCodeSymbolRecord>::new();
    for symbol in symbols {
        index.insert(&symbol.symbol_snapshot_id, symbol);
    }

    index
}

fn caller_for_line<'a>(
    symbols_by_path: &'a BTreeMap<&str, Vec<&'a RepositoryCodeSymbolRecord>>,
    path: &str,
    line: u32,
) -> Option<&'a RepositoryCodeSymbolRecord> {
    symbols_by_path
        .get(path)?
        .iter()
        .copied()
        .filter(|symbol| symbol.line_range.start <= line && symbol.line_range.end >= line)
        .max_by_key(|symbol| symbol.line_range.start)
}

pub(in crate::code) fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        CodeRepositoryRegistration, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    };

    use super::*;

    #[test]
    fn caller_lookup_uses_matching_path_and_innermost_symbol() {
        let symbols = vec![
            symbol("outer", "src/hot.rs", "outer", 1, 10),
            symbol("inner", "src/hot.rs", "inner", 5, 8),
            symbol("other", "src/other.rs", "other", 6, 6),
        ];
        let index = build_symbol_path_index(&symbols);

        let caller = caller_for_line(&index, "src/hot.rs", 6).expect("caller should resolve");

        assert_eq!(caller.name, "inner");
        assert!(caller_for_line(&index, "src/other.rs", 5).is_none());
        assert!(caller_for_line(&index, "src/missing.rs", 6).is_none());
    }

    #[test]
    fn call_materialization_keeps_scoped_hint_and_resolved_callee_name() {
        let registration =
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration");
        let mut build = SnapshotBuild::new(
            &registration,
            "commit".to_owned(),
            "tree".to_owned(),
            true,
            1,
            0,
        );
        build
            .symbols
            .push(symbol("c-definition", "src/c_entry.c", "rk_c_decode", 1, 3));
        build
            .references
            .push(reference("ffi-call", "src/lib.rs", "ffi::rk_c_decode", 2));

        let snapshot = build.finish();
        let call = snapshot.calls.first().expect("call should materialize");

        assert_eq!(call.callee_name, "rk_c_decode");
        assert_eq!(call.target_hint.as_deref(), Some("ffi::rk_c_decode"));
        assert_eq!(
            call.callee_symbol_snapshot_id.as_deref(),
            Some("c-definition")
        );
        assert_eq!(call.resolution_state, "resolved");
    }

    #[test]
    fn indexed_workspace_descendant_scan_respects_directory_and_entry_limits() {
        let entries = (0..8)
            .map(|index| GitTreeEntry {
                path: format!("packages/pkg-{index}/package.json"),
                byte_count: 1,
            })
            .collect::<Vec<_>>();
        let source = IndexedWorkspaceSource {
            root_path: std::path::Path::new("/repo"),
            kind: RepositorySourceKind::FileSystem,
            commit: "commit",
            entries: &entries,
            path_filters: &[],
        };

        assert_eq!(
            source
                .descendant_dirs_containing_file("packages", "package.json", 3, 8)
                .len(),
            3
        );
        assert_eq!(
            source
                .descendant_dirs_containing_file("packages", "package.json", 8, 2)
                .len(),
            2
        );
    }

    fn symbol(
        symbol_snapshot_id: &str,
        path: &str,
        name: &str,
        line_start: u32,
        line_end: u32,
    ) -> RepositoryCodeSymbolRecord {
        RepositoryCodeSymbolRecord {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            symbol_snapshot_id: symbol_snapshot_id.to_owned(),
            canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
            file_id: format!("file-{symbol_snapshot_id}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            name: name.to_owned(),
            qualified_name: format!("{}::{name}", path.replace('/', "::")),
            kind: "function".to_owned(),
            signature: format!("fn {name}()"),
            doc_comment: None,
            byte_range: RepositoryCodeRange { start: 0, end: 1 },
            line_range: RepositoryCodeRange {
                start: line_start,
                end: line_end,
            },
            symbol_role: None,
        }
    }

    fn reference(
        reference_id: &str,
        path: &str,
        name: &str,
        line: u32,
    ) -> RepositoryCodeReferenceRecord {
        RepositoryCodeReferenceRecord {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            reference_id: reference_id.to_owned(),
            file_id: format!("file-{reference_id}"),
            path: path.to_owned(),
            name: name.to_owned(),
            kind: "call".to_owned(),
            target_symbol_snapshot_id: None,
            target_hint: Some(name.to_owned()),
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 2_500,
            confidence_tier: "ambiguous".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 1 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
        }
    }
}
