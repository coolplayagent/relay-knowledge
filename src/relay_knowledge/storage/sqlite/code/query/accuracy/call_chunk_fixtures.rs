use super::*;

pub(super) fn snapshot_with_resolved_callee_tie() -> CodeIndexSnapshot {
    let mut ambiguous = call("ambiguous-callee", "cma-source", "mm/cma_debug.c");
    ambiguous.caller_name = Some("cma_debugfs_init".to_owned());
    ambiguous.callee_name = "debugfs_create_dir".to_owned();
    ambiguous.target_hint = Some("debugfs_create_dir".to_owned());
    ambiguous.line_range = range(205, 205);

    let mut resolved = call("resolved-callee", "cma-source", "mm/cma_debug.c");
    resolved.caller_name = Some("cma_debugfs_init".to_owned());
    resolved.callee_symbol_snapshot_id = Some("cma-debugfs-add-one".to_owned());
    resolved.callee_name = "cma_debugfs_add_one".to_owned();
    resolved.target_hint = Some("cma_debugfs_add_one".to_owned());
    resolved.resolution_state = "resolved".to_owned();
    resolved.confidence_basis_points = 8_000;
    resolved.confidence_tier = "inferred".to_owned();
    resolved.line_range = range(208, 208);

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
            "cma-source",
            "mm/cma_debug.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![symbol(
            "cma-debugfs-add-one",
            "cma-source",
            "mm/cma_debug.c",
            "cma_debugfs_add_one",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![ambiguous, resolved],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_many_caller_candidate_ties() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..550 {
        let file_id = format!("noise-file-{index}");
        let path = format!("src/exactOwner/noise_{index}.py");
        files.push(file(
            &file_id,
            &path,
            "python",
            CodeParseStatus::Parsed,
            None,
        ));
        let mut call = call(&format!("noise-call-{index}"), &file_id, &path);
        call.caller_name = Some(format!("noiseCaller{index}"));
        call.callee_name = "TargetCall".to_owned();
        call.target_hint = Some("TargetCall".to_owned());
        calls.push(call);
    }

    files.push(file(
        "exact-file",
        "src/exact_owner.py",
        "python",
        CodeParseStatus::Parsed,
        None,
    ));
    let mut exact = call("exact-call", "exact-file", "src/exact_owner.py");
    exact.caller_name = Some("exactOwner".to_owned());
    exact.callee_name = "TargetCall".to_owned();
    exact.target_hint = Some("TargetCall".to_owned());
    exact.resolution_state = "resolved".to_owned();
    exact.confidence_basis_points = 8_000;
    exact.confidence_tier = "inferred".to_owned();
    calls.push(exact);

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
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_same_callee_context_noise() -> CodeIndexSnapshot {
    let mut first_noise = call("first-noise-call", "first-noise-file", "src/a_noise.py");
    first_noise.caller_name = Some("otherOwner".to_owned());
    first_noise.callee_name = "TargetCall".to_owned();
    first_noise.target_hint = Some("TargetCall".to_owned());

    let mut second_noise = call("second-noise-call", "second-noise-file", "src/b_noise.py");
    second_noise.caller_name = Some("anotherOwner".to_owned());
    second_noise.callee_name = "TargetCall".to_owned();
    second_noise.target_hint = Some("TargetCall".to_owned());

    let mut exact = call("exact-call", "exact-file", "src/z_exact_owner.py");
    exact.caller_name = Some("exactOwner".to_owned());
    exact.callee_name = "TargetCall".to_owned();
    exact.target_hint = Some("TargetCall".to_owned());

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "first-noise-file",
                "src/a_noise.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "second-noise-file",
                "src/b_noise.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "exact-file",
                "src/z_exact_owner.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![first_noise, second_noise, exact],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_call_site_chunk() -> CodeIndexSnapshot {
    let mut caller = symbol(
        "sanitize-options",
        "db-impl-source",
        "db/db_impl.cc",
        "SanitizeOptions",
    );
    caller.language_id = "cpp".to_owned();
    caller.line_range = range(110, 124);

    let mut call = call("new-lru-cache-call", "db-impl-source", "db/db_impl.cc");
    call.caller_symbol_snapshot_id = Some("sanitize-options".to_owned());
    call.caller_name = Some("SanitizeOptions".to_owned());
    call.callee_name = "NewLRUCache".to_owned();
    call.target_hint = Some("NewLRUCache".to_owned());
    call.resolution_state = "resolved".to_owned();
    call.confidence_basis_points = 8_000;
    call.confidence_tier = "inferred".to_owned();
    call.line_range = range(116, 116);

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
            "db-impl-source",
            "db/db_impl.cc",
            "cpp",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![caller],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            RepositoryCodeChunkRecord {
                line_range: range(110, 115),
                ..chunk(
                    "sanitize-options-prologue",
                    "db-impl-source",
                    "db/db_impl.cc",
                    "Options SanitizeOptions(const Options& src) {\n    Options result;",
                    Some("sanitize-options"),
                )
            },
            RepositoryCodeChunkRecord {
                line_range: range(116, 124),
                ..chunk(
                    "sanitize-options-call-site",
                    "db-impl-source",
                    "db/db_impl.cc",
                    "    result.block_cache = NewLRUCache(8 << 20);\n    return result;\n}",
                    Some("sanitize-options"),
                )
            },
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_eval_checkpoint_chunk() -> CodeIndexSnapshot {
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
            "checkpoint-source",
            "src/relay_teams_evals/checkpoint.py",
            "python",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(
            "checkpoint-chunk",
            "checkpoint-source",
            "src/relay_teams_evals/checkpoint.py",
            "class EvalCheckpointStore:\n    def ensure_initialized(self, signature):\n        raise ValueError(\"Checkpoint signature does not match\")\n\n    def append_result(self, result):\n        self._results_path.write_text(result.model_dump_json())",
            None,
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_cache_interface_chunk_noise() -> CodeIndexSnapshot {
    let target = chunk(
        "cache-interface-chunk",
        "cache-header",
        "include/leveldb/cache.h",
        "class LEVELDB_EXPORT Cache {\n public:\n  virtual Handle* Insert(const Slice& key, void* value, size_t charge,\n                         void (*deleter)(const Slice& key, void* value)) = 0;\n  virtual Handle* Lookup(const Slice& key) = 0;\n  virtual size_t TotalCharge() const = 0;\n};",
        None,
    );
    let noise = chunk(
        "cache-fixture-chunk",
        "cache-fixture",
        "benchmarks/cache_lru_fixture.cc",
        "class CacheFixture {\n public:\n  CacheFixture() : cache_(NewLRUCache(kCacheSize)) {}\n  int Lookup(int key) { return cache_->Lookup(EncodeKey(key)) == nullptr ? -1 : 0; }\n  void Insert(int key, int value, int charge = 1) { cache_->Insert(EncodeKey(key), EncodeValue(value), charge, nullptr); }\n  size_t TotalCharge() const { return cache_->TotalCharge(); }\n};",
        None,
    );

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
                "cache-header",
                "include/leveldb/cache.h",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "cache-fixture",
                "benchmarks/cache_lru_fixture.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![target, noise],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_recovery_manifest_chunk_noise() -> CodeIndexSnapshot {
    let target = chunk(
        "recover-header-chunk",
        "db-impl-header",
        "db/db_impl.h",
        "class DBImpl {\n  // Switches to a new log-file/memtable and writes a new descriptor iff successful.\n  Status RecoverLogFile(uint64_t log_number, bool last_log, bool* save_manifest,\n                        VersionEdit* edit, SequenceNumber* max_sequence)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n  Status WriteLevel0Table(MemTable* mem, VersionEdit* edit, Version* base)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n};",
        None,
    );
    let noise = chunk(
        "recover-implementation-chunk",
        "db-impl-source",
        "db/db_impl.cc",
        "Status DBImpl::RecoverLogFile(uint64_t log_number, bool last_log, bool* save_manifest,\n                              VersionEdit* edit, SequenceNumber* max_sequence) {\n  if (*save_manifest) {\n    descriptor_log_->AddRecord(edit->Encode());\n  }\n  return Status::OK();\n}",
        None,
    );

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
                "db-impl-header",
                "db/db_impl.h",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "db-impl-source",
                "db/db_impl.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![target, noise],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_related_callee_names() -> CodeIndexSnapshot {
    let mut unrelated = call("unmapped-area", "mmap-source", "mm/mmap.c");
    unrelated.caller_name = Some("do_mmap".to_owned());
    unrelated.callee_name = "__get_unmapped_area".to_owned();
    unrelated.target_hint = Some("__get_unmapped_area".to_owned());
    unrelated.resolution_state = "resolved".to_owned();
    unrelated.confidence_basis_points = 8_000;
    unrelated.confidence_tier = "inferred".to_owned();
    unrelated.line_range = range(408, 408);

    let mut related = call("mmap-region", "mmap-source", "mm/mmap.c");
    related.caller_name = Some("do_mmap".to_owned());
    related.callee_name = "mmap_region".to_owned();
    related.target_hint = Some("mmap_region".to_owned());
    related.resolution_state = "resolved".to_owned();
    related.confidence_basis_points = 8_000;
    related.confidence_tier = "inferred".to_owned();
    related.line_range = range(560, 560);

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
            "mmap-source",
            "mm/mmap.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![unrelated, related],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}
