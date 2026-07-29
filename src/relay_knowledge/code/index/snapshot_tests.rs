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
