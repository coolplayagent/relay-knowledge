use std::collections::BTreeMap;

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
        let symbols_by_path = build_symbol_path_index(&self.symbols);
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
                    callee_name: reference.name.clone(),
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
            chunks: self.chunks,
            diagnostics: self.diagnostics,
        }
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

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
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
    use crate::domain::RepositoryCodeRange;

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
        }
    }
}
