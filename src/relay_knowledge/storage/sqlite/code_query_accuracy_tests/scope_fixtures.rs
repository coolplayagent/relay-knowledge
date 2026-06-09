use super::*;

pub(super) fn snapshot_with_target_symbol() -> CodeIndexSnapshot {
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_path_filtered_candidate_overflow() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..600 {
        let file_id = format!("noise-file-{index:03}");
        let path = format!("vendor/noise_{index:03}.rs");
        files.push(file(&file_id, &path, "rust", CodeParseStatus::Parsed, None));
        symbols.push(symbol(
            &format!("noise-symbol-{index:03}"),
            &file_id,
            &path,
            "target",
        ));
    }

    files.push(file(
        "target-file",
        "src/target.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    symbols.push(symbol(
        "target-symbol",
        "target-file",
        "src/target.rs",
        "target",
    ));

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: files.len(),
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_degraded_files(count: usize) -> CodeIndexSnapshot {
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics,
    }
}
