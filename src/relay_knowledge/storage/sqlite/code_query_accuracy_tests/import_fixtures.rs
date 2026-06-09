use super::*;

pub(super) fn snapshot_with_c_imports() -> CodeIndexSnapshot {
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
                "debugfs-header",
                "include/linux/debugfs.h",
                "c",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "cma-source",
                "mm/cma_debug.c",
                "c",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![
            import(
                "debugfs-internal-include",
                "debugfs-header",
                "include/linux/debugfs.h",
                "#include <linux/fs.h>",
                Some("include/linux/fs.h"),
                "unresolved",
            ),
            import(
                "cma-debugfs-include",
                "cma-source",
                "mm/cma_debug.c",
                "#include <linux/debugfs.h>",
                Some("include/linux/debugfs.h"),
                "resolved",
            ),
        ],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_repeated_c_imports() -> CodeIndexSnapshot {
    let mut snapshot = snapshot_with_c_imports();
    snapshot.changed_path_count = 4;
    snapshot.files.extend([
        file(
            "debugfs-file-source",
            "fs/debugfs/file.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        ),
        file(
            "debugfs-inode-source",
            "fs/debugfs/inode.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        ),
    ]);
    let mut file_import = import(
        "debugfs-file-include",
        "debugfs-file-source",
        "fs/debugfs/file.c",
        "#include <linux/debugfs.h>",
        Some("include/linux/debugfs.h"),
        "resolved",
    );
    file_import.line_range = range(16, 16);
    let mut inode_import = import(
        "debugfs-inode-include",
        "debugfs-inode-source",
        "fs/debugfs/inode.c",
        "#include <linux/debugfs.h>",
        Some("include/linux/debugfs.h"),
        "resolved",
    );
    inode_import.line_range = range(23, 23);
    snapshot.imports[1].line_range = range(9, 9);
    snapshot.imports.extend([file_import, inode_import]);

    snapshot
}
