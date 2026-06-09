use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const CASE_INTENT_SOURCE_SCOPE: &str = "code:test:case-intent:commit:tree";

#[path = "code_query_unit_tests/case_intent_tests.rs"]
mod case_intent_tests;

#[test]
fn path_filters_accept_trailing_slashes() {
    assert!(path_matches_filter("src/lib.rs", "src/"));
    assert!(path_matches_filter("src/lib.rs", "src"));
    assert!(path_matches_filter("src/lib.rs", "."));
    assert!(path_matches_filter("src/lib.rs", "./"));
    assert!(path_matches_filter("src/lib.rs", "./src"));
    assert!(!path_matches_filter("src-other/lib.rs", "src/"));
}

#[test]
fn candidate_condition_preserves_all_query_terms() {
    let (condition, values) = candidate_condition(&["lower(name)", "lower(path)"], "retry budget");

    assert!(condition.contains("lower(name) LIKE ?"));
    assert_eq!(values.len(), 4);
    assert!(values.contains(&Value::Text("%retry%".to_owned())));
    assert!(values.contains(&Value::Text("%budget%".to_owned())));
}

#[test]
fn candidate_condition_splits_scoped_queries_for_edge_prefilters() {
    let (_, values) = candidate_condition(&["lower(name)"], "pkg.service.TargetThing");

    assert_eq!(
        values,
        vec![
            Value::Text("%pkg%".to_owned()),
            Value::Text("%service%".to_owned()),
            Value::Text("%targetthing%".to_owned()),
        ]
    );
}

#[test]
fn candidate_condition_caps_bind_values_for_long_queries() {
    let query = (0..300)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let fields = ["a", "b", "c", "d", "e"];

    let (_, values) = candidate_condition(&fields, &query);

    assert!(values.len() <= MAX_CANDIDATE_BIND_VALUES);
}

#[test]
fn symbol_fts_query_uses_any_term_for_fuzzy_recall() {
    let symbol_query = symbol_fts_match_query("checkpoint metadata version constant");
    assert!(
        symbol_query.starts_with("(\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\")")
    );
    assert!(symbol_query.contains("\"checkpointmetadataversionconstant\""));
    assert!(symbol_query.contains("\"metadataversionconstant\""));
    assert_eq!(
        symbol_fts_match_query("new lru cache"),
        "(\"new\" OR \"lru\" OR \"cache\") OR \"newlrucache\" OR \"new_lru_cache\""
    );
    let edge_query = fts_match_query("checkpoint metadata version constant");
    assert!(edge_query.starts_with("(\"checkpoint\" \"metadata\" \"version\" \"constant\")"));
    assert!(edge_query.contains("\"checkpointmetadataversionconstant\""));
    assert!(edge_query.contains("\"checkpoint_metadata_version\""));
    let chunk_query = hybrid_chunk_fts_match_query("checkpoint metadata version constant");
    assert!(
        chunk_query.starts_with("(\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\")")
    );
    assert!(chunk_query.contains("\"checkpointmetadataversionconstant\""));
}

#[test]
fn fts_query_compound_identifier_alternatives_are_bounded() {
    assert_eq!(
        fts_match_query("new lru cache"),
        "(\"new\" \"lru\" \"cache\") OR \"newlrucache\" OR \"new_lru_cache\""
    );
    assert_eq!(fts_match_query("a b"), "\"a\" \"b\"");
    let long_query = fts_match_query(
        "cached introspection results local property handler not writable property exception",
    );
    assert!(long_query.starts_with(
        "(\"cached\" \"introspection\" \"results\" \"local\" \"property\" \"handler\""
    ));
    assert!(long_query.contains("\"cachedintrospectionresultslocal\""));
    assert!(long_query.contains("\"notwritablepropertyexception\""));
    assert!(
        !long_query
            .contains("cachedintrospectionresultslocalpropertyhandlernotwritablepropertyexception")
    );
    assert!(long_query.matches(" OR \"").count() <= 24);
}

#[test]
fn candidate_limits_are_layer_aware_and_bounded_for_repo_set_fanout() {
    let request = candidate_limit_request(30);

    assert_eq!(candidate_limit(&request, CandidateLayer::Symbol), 800);
    assert_eq!(candidate_limit(&request, CandidateLayer::Reference), 700);
    assert_eq!(candidate_limit(&request, CandidateLayer::Call), 800);
    assert_eq!(candidate_limit(&request, CandidateLayer::Import), 700);
    assert_eq!(candidate_limit(&request, CandidateLayer::Chunk), 900);

    let top_k_request = candidate_limit_request(10);
    assert_eq!(candidate_limit(&top_k_request, CandidateLayer::Call), 400);
    assert_eq!(candidate_limit(&top_k_request, CandidateLayer::Chunk), 450);

    let direct_call_request = direct_call_candidate_limit_request(10);
    assert_eq!(
        candidate_limit(&direct_call_request, CandidateLayer::Call),
        1000
    );
}

#[test]
fn plannable_search_outage_returns_existing_partial_hits() {
    let request = code_search_request("rk_handler", CodeQueryKind::Hybrid);
    let mut hits = vec![partial_code_hit(
        "src/lib.rs",
        CodeRetrievalLayer::Symbol,
        4.0,
    )];

    let partial_hits = append_hits_or_return_partial_on_search_outage(
        &mut hits,
        &request,
        Err(code_search_unavailable_error()),
    )
    .expect("plannable read-model outage should return partial hits")
    .expect("partial hits should be available");

    assert!(hits.is_empty());
    assert_eq!(partial_hits.len(), 1);
    assert_eq!(partial_hits[0].path, "src/lib.rs");
    assert!(
        partial_hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Symbol)
    );
    assert_read_model_degraded(&partial_hits[0]);
}

#[test]
fn non_plannable_search_outage_propagates_instead_of_returning_partial_hits() {
    let request = code_search_request("find rk_handler", CodeQueryKind::Hybrid);
    let mut hits = vec![partial_code_hit(
        "src/lib.rs",
        CodeRetrievalLayer::Symbol,
        4.0,
    )];

    let result = append_hits_or_return_partial_on_search_outage(
        &mut hits,
        &request,
        Err(code_search_unavailable_error()),
    );

    assert!(result.is_err());
    assert_eq!(hits.len(), 1);
    assert!(hits[0].degraded_reason.is_none());
}

#[tokio::test]
async fn reference_identity_hits_report_read_model_outage_when_fts_unavailable() {
    let mut snapshot = code_query_snapshot(
        vec![code_query_file("reference-file", "src/lib.rs", "rust")],
        Vec::new(),
        Vec::new(),
    );
    snapshot.references.push(code_query_reference(
        "reference-foo",
        "reference-file",
        "src/lib.rs",
        "foo",
    ));
    let store = store_with_case_intent_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let hits = store
        .search_code(code_search_request("foo", CodeQueryKind::References))
        .await
        .expect("reference identity hits should survive FTS outage");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Reference)
    );
    assert_read_model_degraded(&hits[0]);
}

#[tokio::test]
async fn chunk_first_hybrid_api_hits_report_read_model_outage_when_fts_unavailable() {
    let snapshot = code_query_snapshot(
        vec![code_query_file("api-file", "src/worker.rs", "rust")],
        vec![
            code_query_symbol(
                "register-workflow",
                "api-file",
                "src/worker.rs",
                "RegisterWorkflow",
            ),
            code_query_symbol(
                "register-activity",
                "api-file",
                "src/worker.rs",
                "RegisterActivity",
            ),
        ],
        Vec::new(),
    );
    let store = store_with_case_intent_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let hits = store
        .search_code(code_search_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("chunk-first hybrid API hits should survive FTS outage");

    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| {
        hit.path == "src/worker.rs" && hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
    }));
    assert!(hits.iter().all(|hit| hit.degraded_reason.is_some()));
    assert_read_model_degraded(&hits[0]);
}

#[test]
fn score_text_matches_identifier_parts_inside_snake_case_names() {
    let score = score_text(
        "archive output directory",
        ["def archive_output_dir(output_dir: Path) -> Path:"],
    );

    assert!(score >= 4.0);
    assert_eq!(score_text("service ip range", ["getServiceIPRanges"]), 6.0);
    assert_eq!(
        score_text("bloom filter policies", ["NewBloomFilterPolicy"]),
        6.0
    );
    assert_eq!(score_text("status", ["statu"]), 0.0);
}

#[test]
fn score_text_preserves_exact_match_ceiling_after_identifier_match() {
    assert_eq!(score_text("cache", ["block_cache", "cache"]), 4.0);
    assert_eq!(score_text("cache", ["block_cache"]), 2.0);
    assert_eq!(score_text("cach", ["block_cache"]), 0.5);
}

#[test]
fn declaration_chunk_bonus_requires_declaration_shape() {
    let terms = query_terms("recover descriptor save_manifest versionedit");

    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "Status DBImpl::RecoverLogFile(uint64_t log_number, bool* save_manifest) {\n  descriptor_log_->AddRecord(edit->Encode());\n}"
        ),
        0.0
    );
    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "class DBImpl {\n  Status RecoverLogFile(uint64_t log_number, bool* save_manifest,\n                        VersionEdit* edit)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n  Status WriteLevel0Table(MemTable* mem, VersionEdit* edit)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n};"
        ),
        2.0
    );
}

#[test]
fn declaration_chunk_bonus_preserves_interface_boost() {
    let terms = query_terms("cache interface lookup insert total charge lru");

    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "class Cache {\n public:\n  virtual Handle* Insert(const Slice& key, void* value, size_t charge) = 0;\n  virtual Handle* Lookup(const Slice& key) = 0;\n  virtual size_t TotalCharge() const = 0;\n};"
        ),
        3.0
    );
}

#[test]
fn declaration_chunk_bonus_accepts_mixin_and_parenthesized_inheritance_surfaces() {
    let ruby_terms = query_terms("module mixin controller runtime normalize event dispatch");
    let python_terms = query_terms("service overload exception subclass normalize payload");
    let interface_terms = query_terms("interfaces cache lookup adapter surface");

    assert_eq!(
        declaration_chunk_bonus(
            &ruby_terms,
            "module Extensions\n  def normalize_event(event)\n    event.to_s.strip\n  end\nend",
        ),
        4.75
    );
    assert_eq!(
        declaration_chunk_bonus(
            &python_terms,
            "class OverloadedServiceError(ServiceError):\n    pass",
        ),
        2.75
    );
    assert_eq!(
        declaration_chunk_bonus(
            &interface_terms,
            "interface CacheAdapter {\n  lookup(key: string): CacheEntry\n}",
        ),
        4.75
    );
}

#[test]
fn scoped_identity_matches_over_nested_owner_scopes() {
    let identity = SymbolIdentityQuery::from_query("SessionClient.request")
        .expect("scoped identity should parse");

    assert!(identity.matches_symbol(
        "request",
        "Sources::App::SessionClient::SessionClient.init.request.request",
        "func request(url: URL) async throws -> Data {",
        "repo://repo/Sources::App::SessionClient::SessionClient.init.request.request",
    ));
    assert!(!identity.matches_symbol(
        "send",
        "Sources::App::SessionClient::SessionTransport.send",
        "func send(_ request: URLRequest) async throws -> Data",
        "repo://repo/Sources::App::SessionClient::SessionTransport.send",
    ));
}

#[test]
fn import_surface_bonus_prefers_public_reexport_files() {
    assert_eq!(
        import_surface_bonus(0.0, "src/pkg/__init__.py", CodeQueryKind::Hybrid),
        0.0
    );
    assert!(import_surface_bonus(3.0, "src/pkg/__init__.py", CodeQueryKind::Hybrid) > 0.0);
    assert!(import_surface_bonus(3.0, "src/lib.rs", CodeQueryKind::Hybrid) > 0.0);
    assert!(import_surface_bonus(3.0, "src/index.ts", CodeQueryKind::Hybrid) > 0.0);
    assert_eq!(
        import_surface_bonus(3.0, "src/pkg/__init__.py", CodeQueryKind::Imports),
        0.0
    );
    assert_eq!(
        import_surface_bonus(3.0, "tests/pkg/__init__.py", CodeQueryKind::Hybrid),
        0.0
    );
    assert_eq!(
        import_surface_bonus(3.0, "tests/pkg/test_imports.py", CodeQueryKind::Hybrid),
        0.0
    );
}

#[test]
fn import_path_queries_prefer_consuming_source_paths_over_header_forwarders() {
    assert!(
        import_public_dependency_surface_bonus(
            3.0,
            "leveldb/filter_policy.h",
            "db/dbformat.h",
            Some("include/leveldb/filter_policy.h"),
            CodeQueryKind::Imports,
        ) > 0.0
    );
    assert!(
        import_source_path_query_overlap_bonus(
            3.0,
            "leveldb/filter_policy.h",
            "table/filter_block.cc",
            Some("include/leveldb/filter_policy.h"),
            CodeQueryKind::Imports,
        ) > 0.0
    );
    assert!(
        import_self_implementation_penalty(
            3.0,
            "leveldb/filter_policy.h",
            "util/filter_policy.cc",
            Some("include/leveldb/filter_policy.h"),
            CodeQueryKind::Imports,
        ) < 0.0
    );
}

#[test]
fn import_queries_demote_package_reexport_surfaces() {
    assert!(
        import_reexport_surface_penalty(
            3.0,
            "relay_teams.connector.models",
            "src/relay_teams/connector/__init__.py",
            "from relay_teams.connector.models import Connector",
            Some("src/relay_teams/connector/models.py"),
            CodeQueryKind::Imports,
        ) < 0.0
    );
    assert_eq!(
        import_reexport_surface_penalty(
            3.0,
            "relay_teams.connector.models",
            "src/relay_teams/connector/service.py",
            "from relay_teams.connector.models import Connector",
            Some("src/relay_teams/connector/models.py"),
            CodeQueryKind::Imports,
        ),
        0.0
    );
}

#[test]
fn import_target_symbol_bonus_matches_fully_qualified_class_tail() {
    assert_eq!(
        import_target_symbol_bonus(
            "org.springframework.context.ApplicationContext",
            Some("ApplicationContext"),
        ),
        2.0
    );
    assert_eq!(
        import_target_symbol_bonus("org.springframework.context", Some("ApplicationContext")),
        0.0
    );
    assert_eq!(import_target_symbol_bonus("ApplicationContext", None), 0.0);
}

#[test]
fn target_symbol_import_query_skips_path_like_queries() {
    assert!(target_symbol_import_query("SharedInformerFactory"));
    assert!(target_symbol_import_query("DefaultListableBeanFactory"));
    assert!(target_symbol_import_query(
        "org.springframework.context.ApplicationContext"
    ));
    assert!(!target_symbol_import_query("linux/debugfs.h"));
    assert!(!target_symbol_import_query("linux.debugfs.h"));
    assert!(!target_symbol_import_query("src\\debugfs.h"));
    assert!(!target_symbol_import_query(
        "DefaultListableBeanFactory.java"
    ));
}

#[test]
fn symbol_name_bonus_splits_query_identifiers_for_hybrid_context() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let callers = retrieval_request(CodeQueryKind::Callers);

    assert_eq!(
        symbol_name_query_bonus(
            "EvalCheckpointStore signature mismatch append result",
            "EvalCheckpointStore",
            &hybrid,
        ),
        2.0
    );
    assert_eq!(
        symbol_name_query_bonus("bloom filter policies", "NewBloomFilterPolicy", &hybrid),
        2.0
    );
    assert!(
        symbol_name_query_bonus(
            "checkpoint metadata version constant",
            "_CHECKPOINT_VERSION",
            &hybrid,
        ) > symbol_name_query_bonus(
            "checkpoint metadata version constant",
            "FEISHU_METADATA_PLATFORM_KEY",
            &hybrid,
        )
    );
    assert_eq!(
        symbol_name_query_bonus(
            "checkpoint metadata version constant",
            "_CHECKPOINT_VERSION",
            &callers,
        ),
        0.0
    );
}

#[test]
fn symbol_query_bonus_prefers_stream_connection_lifecycle_openers() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let query = "background stream discovery reconcile multiplex run event source reconnect";

    let opener_bonus = symbol_query_bonus(
        query,
        "openRunStreamConnection",
        "frontend.stream.openRunStreamConnection",
        "function openRunStreamConnection(connection, { reason, afterEventId = null } = {})",
        "repo://frontend/stream/openRunStreamConnection",
        &hybrid,
    );
    let incidental_helper_bonus = symbol_query_bonus(
        query,
        "runBackgroundDiscovery",
        "frontend.stream.runBackgroundDiscovery",
        "async function runBackgroundDiscovery()",
        "repo://frontend/stream/runBackgroundDiscovery",
        &hybrid,
    );

    assert!(opener_bonus > incidental_helper_bonus, "{opener_bonus}");
}

#[test]
fn symbol_query_bonus_prefers_common_chunk_conversion_adapters() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let query = "provider responses tool calls convert common chunk";

    let adapter_bonus = symbol_query_bonus(
        query,
        "fromProviderChunk",
        "provider.fromProviderChunk",
        "export function fromProviderChunk(chunk: string): CommonChunk | string {",
        "repo://provider/fromProviderChunk",
        &hybrid,
    );
    let type_guard_bonus = symbol_query_bonus(
        query,
        "isResponseFunctionCallArgumentsDeltaChunk",
        "provider.isResponseFunctionCallArgumentsDeltaChunk",
        "function isResponseFunctionCallArgumentsDeltaChunk(chunk: ProviderChunk): chunk is ResponseDeltaChunk {",
        "repo://provider/isResponseFunctionCallArgumentsDeltaChunk",
        &hybrid,
    );

    assert!(adapter_bonus > type_guard_bonus, "{adapter_bonus}");

    let provider_event_bonus = symbol_query_bonus(
        "shared provider events transform response parts",
        "fromProviderEvent",
        "provider.fromProviderEvent",
        "export function fromProviderEvent(event: SharedProviderEvent): ResponsePart {",
        "repo://provider/fromProviderEvent",
        &hybrid,
    );
    let provider_guard_bonus = symbol_query_bonus(
        "shared provider events transform response parts",
        "isProviderEvent",
        "provider.isProviderEvent",
        "function isProviderEvent(event: unknown): event is SharedProviderEvent {",
        "repo://provider/isProviderEvent",
        &hybrid,
    );

    assert!(
        provider_event_bonus > provider_guard_bonus,
        "{provider_event_bonus}"
    );
}

#[test]
fn symbol_excerpt_adds_class_owner_for_member_context() {
    assert_eq!(
        symbol_excerpt(
            "append_result",
            "src::relay_teams_evals::checkpoint::EvalCheckpointStore.append_result",
            "def append_result(self, result: EvalResult) -> None:",
            None,
        ),
        "EvalCheckpointStore.append_result: def append_result(self, result: EvalResult) -> None:"
    );
    assert_eq!(
        symbol_excerpt(
            "archive_output_dir",
            "src::relay_teams_evals::checkpoint::archive_output_dir",
            "def archive_output_dir(output_dir: Path) -> Path:",
            None,
        ),
        "def archive_output_dir(output_dir: Path) -> Path:"
    );
    assert_eq!(
        symbol_excerpt(
            "Compare",
            "leveldb::InternalKeyComparator::Compare",
            "virtual int Compare(const Slice& a, const Slice& b) const;",
            Some("Comparator interface"),
        ),
        "InternalKeyComparator.Compare: Comparator interface\nvirtual int Compare(const Slice& a, const Slice& b) const;"
    );
}

fn retrieval_request(kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector should be valid");

    CodeRetrievalRequest::new(
        "checkpoint metadata version constant",
        selector,
        kind,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}

fn code_search_request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");

    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should be valid")
}

fn code_search_unavailable_error() -> StorageError {
    StorageError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some("no such table: code_repository_search".to_owned()),
    ))
}

fn partial_code_hit(path: &str, layer: CodeRetrievalLayer, score: f64) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: CASE_INTENT_SOURCE_SCOPE.to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        byte_range: code_query_range(0, 10),
        line_range: code_query_range(1, 1),
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some(format!("repo://repo/{}", path.replace('/', "::"))),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![layer],
        index_versions: vec!["code:commit".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: "fn rk_handler() {}".to_owned(),
    }
}

fn assert_read_model_degraded(hit: &CodeRetrievalHit) {
    let reason = hit
        .degraded_reason
        .as_deref()
        .expect("partial hit should report read-model degradation");
    assert!(reason.contains("code search read model unavailable"));
    assert!(reason.contains("code_repository_search"));
}

fn candidate_limit_request(limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");

    CodeRetrievalRequest::new(
        "target",
        selector,
        CodeQueryKind::Hybrid,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}

fn direct_call_candidate_limit_request(limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");

    CodeRetrievalRequest::new(
        "target",
        selector,
        CodeQueryKind::Callers,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}

fn code_query_snapshot(
    files: Vec<RepositoryCodeFileRecord>,
    symbols: Vec<RepositoryCodeSymbolRecord>,
    calls: Vec<CodeCallRecord>,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: CASE_INTENT_SOURCE_SCOPE.to_owned(),
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn code_query_file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: CASE_INTENT_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn code_query_symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: CASE_INTENT_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "python".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("def {name}():"),
        doc_comment: None,
        byte_range: code_query_range(0, 1),
        line_range: code_query_range(1, 1),
    }
}

fn code_query_reference(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: CASE_INTENT_SOURCE_SCOPE.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "read".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: code_query_range(0, 3),
        line_range: code_query_range(1, 1),
    }
}

fn code_query_call(call_id: &str, file_id: &str, path: &str) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: CASE_INTENT_SOURCE_SCOPE.to_owned(),
        call_id: call_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: None,
        callee_symbol_snapshot_id: None,
        callee_name: "target".to_owned(),
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        line_range: code_query_range(1, 1),
    }
}

fn code_query_range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_case_intent_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
