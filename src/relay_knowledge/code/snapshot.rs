use crate::domain::{
    CodeCallRecord, CodeIndexSnapshot, CodePathTombstone, CodeRepositoryRegistration,
    CodeRepositorySelector, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    code_snapshot_scope_id,
};

use super::{identity, ids::stable_id};

pub(super) struct SnapshotBuild {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) base_resolved_commit_sha: Option<String>,
    pub(super) commit: String,
    pub(super) tree_hash: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    full_replace: bool,
    changed_path_count: usize,
    pub(super) skipped_unchanged_count: usize,
    pub(super) deleted_paths: Vec<String>,
    pub(super) tombstones: Vec<CodePathTombstone>,
    pub(super) files: Vec<crate::domain::RepositoryCodeFileRecord>,
    pub(super) symbols: Vec<RepositoryCodeSymbolRecord>,
    pub(super) references: Vec<RepositoryCodeReferenceRecord>,
    pub(super) imports: Vec<crate::domain::CodeImportRecord>,
    calls: Vec<CodeCallRecord>,
    pub(super) chunks: Vec<crate::domain::RepositoryCodeChunkRecord>,
    pub(super) diagnostics: Vec<crate::domain::CodeFileDiagnostic>,
}

impl SnapshotBuild {
    #[cfg(test)]
    pub(super) fn new(
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

    pub(super) fn new_with_selector(
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
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn finish(mut self) -> CodeIndexSnapshot {
        identity::enrich_symbol_identities(&self.repository_id, &mut self.symbols);
        identity::resolve_reference_targets(&self.symbols, &mut self.references);
        identity::resolve_import_targets(&self.files, &self.symbols, &mut self.imports);
        self.calls = self
            .references
            .iter()
            .filter(|reference| reference.kind == "call")
            .map(|reference| CodeCallRecord {
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
                callee_symbol_snapshot_id: reference.target_symbol_snapshot_id.clone(),
                callee_name: reference.name.clone(),
                target_hint: reference.target_hint.clone(),
                resolution_state: reference.resolution_state.clone(),
                confidence_basis_points: reference.confidence_basis_points,
                confidence_tier: reference.confidence_tier.clone(),
                line_range: reference.line_range.clone(),
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
            chunks: self.chunks,
            diagnostics: self.diagnostics,
        }
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

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}
