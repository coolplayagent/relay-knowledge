use super::*;

pub(super) fn snapshot_with_exact_match_noise() -> CodeIndexSnapshot {
    let mut exact_callee = call("exact-callee", "connector-file", "src/connector.py");
    exact_callee.caller_name = Some("list_connectors".to_owned());
    exact_callee.callee_name = "_summary".to_owned();
    exact_callee.target_hint = Some("_summary".to_owned());
    exact_callee.line_range = range(10, 10);

    let mut noisy_callee = call("noisy-callee", "agent-file", "src/agent.py");
    noisy_callee.caller_name = Some("agent_runtimes_list".to_owned());
    noisy_callee.callee_name = "_render_agent_summary_table".to_owned();
    noisy_callee.target_hint = Some("_render_agent_summary_table".to_owned());
    noisy_callee.line_range = range(5, 5);

    let mut exact_caller = call("exact-caller", "summary-file", "src/summary.py");
    exact_caller.caller_name = Some("_summary".to_owned());
    exact_caller.callee_name = "ConnectorSummary".to_owned();
    exact_caller.target_hint = Some("ConnectorSummary".to_owned());
    exact_caller.line_range = range(20, 20);

    let mut noisy_caller = call("noisy-caller", "agent-file", "src/agent.py");
    noisy_caller.caller_name = Some("_render_agent_summary_table".to_owned());
    noisy_caller.callee_name = "echo".to_owned();
    noisy_caller.target_hint = Some("echo".to_owned());
    noisy_caller.line_range = range(6, 6);

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
            file(
                "connector-file",
                "src/connector.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "agent-file",
                "src/agent.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "summary-file",
                "src/summary.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "builder-file",
                "tests/builder.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![
            symbol(
                "exact-builder",
                "builder-file",
                "tests/builder.py",
                "_build_service",
            ),
            symbol(
                "noisy-builder",
                "builder-file",
                "tests/builder.py",
                "_build_service_with_control",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![exact_callee, noisy_callee, exact_caller, noisy_caller],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_type_name_signature_mentions() -> CodeIndexSnapshot {
    let mut request_type = symbol(
        "w3-save-request",
        "w3-models-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
    );
    request_type.language_id = "python".to_owned();
    request_type.kind = "class".to_owned();
    request_type.signature = "class W3ConnectorSaveRequest(BaseModel):".to_owned();

    let mut save_method = symbol(
        "save-w3-connector",
        "service-file",
        "src/relay_teams/connector/service.py",
        "save_w3_connector",
    );
    save_method.language_id = "python".to_owned();
    save_method.kind = "method".to_owned();
    save_method.signature =
        "async def save_w3_connector(self, request: W3ConnectorSaveRequest)".to_owned();

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
                "w3-models-file",
                "src/relay_teams/connector/w3_models.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "service-file",
                "src/relay_teams/connector/service.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![save_method, request_type],
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

pub(super) fn snapshot_with_many_signature_mentions() -> CodeIndexSnapshot {
    let mut request_type = symbol(
        "w3-save-request",
        "w3-models-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
    );
    request_type.language_id = "python".to_owned();
    request_type.kind = "class".to_owned();
    request_type.signature = "class W3ConnectorSaveRequest(BaseModel):".to_owned();
    request_type.line_range = range(1_000, 1_000);

    let mut symbols = Vec::new();
    for index in 0..550 {
        let mut save_method = symbol(
            &format!("save-w3-connector-{index}"),
            "service-file",
            "src/relay_teams/connector/service.py",
            &format!("save_w3_connector_{index}"),
        );
        save_method.language_id = "python".to_owned();
        save_method.kind = "method".to_owned();
        save_method.signature =
            format!("async def save_w3_connector_{index}(self, request: W3ConnectorSaveRequest)");
        save_method.line_range = range(index + 1, index + 1);
        symbols.push(save_method);
    }
    symbols.push(request_type);

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
                "w3-models-file",
                "src/relay_teams/connector/w3_models.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "service-file",
                "src/relay_teams/connector/service.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
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

pub(super) fn snapshot_with_scoped_cpp_definition_noise() -> CodeIndexSnapshot {
    let mut db_open = symbol("db-open", "db-impl-source", "db/db_impl.cc", "Open");
    db_open.language_id = "cpp".to_owned();
    db_open.qualified_name = "leveldb.DB.Open".to_owned();
    db_open.signature =
        "Status DB::Open(const Options& options, const std::string& dbname, DB** dbptr)".to_owned();
    db_open.line_range = range(1503, 1503);

    let mut open_db = symbol(
        "open-db-helper",
        "fault-injection-source",
        "db/fault_injection_test.cc",
        "OpenDB",
    );
    open_db.language_id = "cpp".to_owned();
    open_db.qualified_name = "leveldb.FaultInjectionTest.OpenDB".to_owned();
    open_db.signature = "Status OpenDB()".to_owned();
    open_db.line_range = range(453, 458);

    let mut write_batch_put = symbol(
        "write-batch-put",
        "write-batch-source",
        "db/write_batch.cc",
        "Put",
    );
    write_batch_put.language_id = "cpp".to_owned();
    write_batch_put.qualified_name = "leveldb.WriteBatch.Put".to_owned();
    write_batch_put.signature =
        "void WriteBatch::Put(const Slice& key, const Slice& value)".to_owned();
    write_batch_put.line_range = range(98, 98);

    let mut c_wrapper = symbol(
        "c-wrapper-put",
        "c-source",
        "db/c.cc",
        "leveldb_writebatch_put",
    );
    c_wrapper.language_id = "cpp".to_owned();
    c_wrapper.signature =
        "void leveldb_writebatch_put(leveldb_writebatch_t* b, const char* key, size_t klen)"
            .to_owned();
    c_wrapper.line_range = range(332, 335);

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
            file(
                "db-impl-source",
                "db/db_impl.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "fault-injection-source",
                "db/fault_injection_test.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "write-batch-source",
                "db/write_batch.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file("c-source", "db/c.cc", "cpp", CodeParseStatus::Parsed, None),
        ],
        symbols: vec![open_db, db_open, c_wrapper, write_batch_put],
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
