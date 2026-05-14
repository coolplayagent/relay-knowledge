use crate::domain::{CodeCallRecord, CodeFileDiagnostic, CodeIndexSnapshot, CodeParseStatus};

use super::code_test_support::{
    TEST_SOURCE_SCOPE, call, chunk, file, import, import_module, reference, symbol,
};

pub(in crate::storage::sqlite::code) fn retarget_snapshot_scope(
    snapshot: &mut CodeIndexSnapshot,
    source_scope: &str,
) {
    snapshot.source_scope = source_scope.to_owned();
    for file in &mut snapshot.files {
        file.source_scope = source_scope.to_owned();
    }
    for symbol in &mut snapshot.symbols {
        symbol.source_scope = source_scope.to_owned();
    }
    for reference in &mut snapshot.references {
        reference.source_scope = source_scope.to_owned();
    }
    for import in &mut snapshot.imports {
        import.source_scope = source_scope.to_owned();
    }
    for call in &mut snapshot.calls {
        call.source_scope = source_scope.to_owned();
    }
    for chunk in &mut snapshot.chunks {
        chunk.source_scope = source_scope.to_owned();
    }
    for diagnostic in &mut snapshot.diagnostics {
        diagnostic.source_scope = source_scope.to_owned();
    }
    for tombstone in &mut snapshot.tombstones {
        tombstone.source_scope = source_scope.to_owned();
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_chunk(
    repository_id: &str,
    path: &str,
    content: &str,
) -> CodeIndexSnapshot {
    snapshot_with_chunk_status(repository_id, path, content, CodeParseStatus::Parsed, None)
}

pub(in crate::storage::sqlite::code) fn snapshot_with_chunk_status(
    repository_id: &str,
    path: &str,
    content: &str,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "file",
            path,
            "rust",
            parse_status,
            degraded_reason.clone(),
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk("chunk", "file", path, content, None)],
        diagnostics: degraded_reason
            .map(|message| CodeFileDiagnostic {
                repository_id: repository_id.to_owned(),
                source_scope: TEST_SOURCE_SCOPE.to_owned(),
                path: path.to_owned(),
                parse_status,
                message,
            })
            .into_iter()
            .collect(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_symbol_and_matching_chunk()
-> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "target-file",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![symbol(
            "target-symbol",
            "target-file",
            "src/lib.rs",
            "target",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "target-chunk",
            "target-file",
            "src/lib.rs",
            "fn target()",
            Some("target-symbol"),
        )],
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_degraded_files(
    count: usize,
) -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    for index in 0..count {
        let file_id = format!("file-{index}");
        let path = format!("src/degraded_{index}.rs");
        let message = format!("parse degraded {index}");
        files.push(file(
            &file_id,
            &path,
            "rust",
            CodeParseStatus::Partial,
            Some(message.clone()),
        ));
        diagnostics.push(CodeFileDiagnostic {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            path,
            parse_status: CodeParseStatus::Partial,
            message,
        });
    }

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: count,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics,
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_language_edges() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "rust-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "python-file",
                "py/app.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: vec![
            reference("rust-reference", "rust-file", "src/lib.rs", None),
            reference("python-reference", "python-file", "py/app.py", None),
        ],
        imports: vec![
            import("rust-import", "rust-file", "src/lib.rs"),
            import("python-import", "python-file", "py/app.py"),
        ],
        calls: vec![
            call("rust-call", "rust-file", "src/lib.rs", None),
            call("python-call", "python-file", "py/app.py", None),
        ],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_resolved_reference() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "target-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "caller-file",
                "src/caller.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![symbol(
            "target-symbol",
            "target-file",
            "src/lib.rs",
            "target",
        )],
        references: vec![reference(
            "target-reference",
            "caller-file",
            "src/caller.rs",
            Some("target-symbol"),
        )],
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_duplicate_callee_names() -> CodeIndexSnapshot
{
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("a-file", "src/a.rs", "rust", CodeParseStatus::Parsed, None),
            file("b-file", "src/b.rs", "rust", CodeParseStatus::Parsed, None),
            file(
                "caller-a-file",
                "src/caller_a.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "caller-b-file",
                "src/caller_b.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![
            symbol("target-a", "a-file", "src/a.rs", "target"),
            symbol("target-b", "b-file", "src/b.rs", "target"),
            symbol("caller-a", "caller-a-file", "src/caller_a.rs", "caller"),
            symbol("caller-b", "caller-b-file", "src/caller_b.rs", "caller"),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![
            call_with_caller(
                "call-a",
                "caller-a-file",
                "src/caller_a.rs",
                "caller-a",
                Some("target-a"),
            ),
            call_with_caller(
                "call-b",
                "caller-b-file",
                "src/caller_b.rs",
                "caller-b",
                Some("target-b"),
            ),
        ],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn call_with_caller(
    id: &str,
    file_id: &str,
    path: &str,
    caller_symbol_snapshot_id: &str,
    callee_symbol_snapshot_id: Option<&str>,
) -> CodeCallRecord {
    let mut record = call(id, file_id, path, callee_symbol_snapshot_id);
    record.caller_symbol_snapshot_id = Some(caller_symbol_snapshot_id.to_owned());
    record
}

pub(in crate::storage::sqlite::code) fn snapshot_with_out_of_scope_seed() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "out-file",
                "tests/out.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "caller-file",
                "src/caller.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![symbol("out-target", "out-file", "tests/out.rs", "target")],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call(
            "out-call",
            "caller-file",
            "src/caller.rs",
            Some("out-target"),
        )],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_rust_symbol_importer() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "lib-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "main-file",
                "src/main.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![symbol(
            "retry-symbol",
            "lib-file",
            "src/lib.rs",
            "retry_policy",
        )],
        references: Vec::new(),
        imports: vec![import_module(
            "main-import",
            "main-file",
            "src/main.rs",
            "use crate::retry_policy;",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_deleted_rust_module_importer()
-> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "src/caller.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![import_module(
            "caller-import",
            "caller-file",
            "src/caller.rs",
            "use crate::deleted;",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_deleted_go_module_importer()
-> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "caller.go",
            "go",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![import_module(
            "caller-import",
            "caller-file",
            "caller.go",
            "import \"deleted\"",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_unresolved_caller() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "src/caller.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call("call", "caller-file", "src/caller.rs", None)],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(in crate::storage::sqlite::code) fn snapshot_with_degraded_and_parsed_files()
-> CodeIndexSnapshot {
    let mut snapshot = snapshot_with_chunk_status(
        "repo",
        "README.txt",
        "RetryPolicy appears in docs",
        CodeParseStatus::TextOnly,
        Some("tree-sitter grammar is not configured".to_owned()),
    );
    snapshot.files.push(file(
        "src-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.chunks.push(chunk(
        "src-chunk",
        "src-file",
        "src/lib.rs",
        "fn kept() {}",
        None,
    ));
    snapshot
}

pub(in crate::storage::sqlite::code) fn incremental_snapshot_for_parsed_file() -> CodeIndexSnapshot
{
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: Some("commit".to_owned()),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree-2".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "src-file-2",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "src-chunk-2",
            "src-file-2",
            "src/lib.rs",
            "fn kept() -> u32 { 1 }",
            None,
        )],
        diagnostics: Vec::new(),
    }
}
