use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const CASE_INTENT_SOURCE_SCOPE: &str = "code:test:case-intent:commit:tree";

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
fn score_query_preserves_score_text_semantics() {
    let query = "Cache archiveOutput";
    let fields = ["block_cache", "def archive_output_dir() -> Path:"];

    assert_eq!(
        ScoreQuery::new(query).score(fields),
        score_text(query, fields)
    );
    assert_eq!(ScoreQuery::new("   ").score(["anything"]), 0.0);
}

#[test]
fn score_query_preserves_multi_token_identifier_scores() {
    let score = ScoreQuery::new("cache output archive").score([
        "block_cache",
        "archiveOutput",
        "def archive_output_dir() -> Path:",
    ]);

    assert_eq!(score, 6.0);
}

#[test]
fn scoped_identity_query_bonus_matches_qualified_edge_targets() {
    assert_eq!(
        scoped_identity_query_bonus(
            "pkg.service.TargetThing",
            ["repo://example/src::pkg::service::TargetThing"],
        ),
        2.0
    );
    assert_eq!(
        scoped_identity_query_bonus("TargetThing", ["pkg.service.TargetThing"]),
        0.0
    );
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
fn import_surface_bonus_prefers_public_reexport_files() {
    assert_eq!(import_surface_bonus(0.0, "src/pkg/__init__.py"), 0.0);
    assert!(import_surface_bonus(3.0, "src/pkg/__init__.py") > 0.0);
    assert!(import_surface_bonus(3.0, "src/lib.rs") > 0.0);
    assert!(import_surface_bonus(3.0, "src/index.ts") > 0.0);
    assert_eq!(import_surface_bonus(3.0, "tests/pkg/__init__.py"), 0.0);
    assert_eq!(import_surface_bonus(3.0, "tests/pkg/test_imports.py"), 0.0);
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

#[tokio::test]
async fn symbol_search_preserves_case_for_name_bonus() {
    let mut target = code_query_symbol(
        "eval-checkpoint-store",
        "checkpoint-file",
        "src/relay_teams_evals/checkpoint.py",
        "EvalCheckpointStore",
    );
    target.kind = "class".to_owned();
    target.signature = "class EvalCheckpointStore:".to_owned();
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file(
            "checkpoint-file",
            "src/relay_teams_evals/checkpoint.py",
            "python",
        )],
        vec![target],
        Vec::new(),
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "EvalCheckpointStore signature mismatch append result",
            CodeQueryKind::Definition,
        ))
        .await
        .expect("symbol query should succeed");

    let hit = hits
        .iter()
        .find(|hit| hit.symbol_snapshot_id.as_deref() == Some("eval-checkpoint-store"))
        .expect("target symbol should be recalled");
    let lower_hits = store
        .search_code(code_search_request(
            "evalcheckpointstore signature mismatch append result",
            CodeQueryKind::Definition,
        ))
        .await
        .expect("lowercase symbol query should succeed");
    let spaced_hits = store
        .search_code(code_search_request(
            "eval checkpoint store",
            CodeQueryKind::Definition,
        ))
        .await
        .expect("spaced compound symbol query should succeed");
    assert_eq!(
        spaced_hits[0].symbol_snapshot_id.as_deref(),
        Some("eval-checkpoint-store")
    );
    assert!(
        hit.score > lower_hits[0].score + 1.5,
        "mixed-case query should keep CamelCase symbol-name bonus, got {} vs lowercase {}",
        hit.score,
        lower_hits[0].score
    );
}

#[tokio::test]
async fn symbol_search_pushes_language_filters_before_candidate_limit() {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..550 {
        let file_id = format!("noise-file-{index:03}");
        let path = format!("pkg/noise_{index:03}.py");
        files.push(code_query_file(&file_id, &path, "python"));
        symbols.push(code_query_symbol(
            &format!("noise-symbol-{index:03}"),
            &file_id,
            &path,
            "target",
        ));
    }

    files.push(code_query_file("target-file", "src/lib.rs", "rust"));
    let mut target = code_query_symbol("target-symbol", "target-file", "src/lib.rs", "target");
    target.language_id = "rust".to_owned();
    target.signature = "fn target() {}".to_owned();
    symbols.push(target);
    let store =
        store_with_case_intent_snapshot(code_query_snapshot(files, symbols, Vec::new())).await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");

    let hits = store
        .search_code(
            CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Definition,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("language-filtered symbol query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].language_id, "rust");
}

#[tokio::test]
async fn symbol_search_preserves_case_for_test_intent() {
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file(
            "coverage-file",
            "tests/unit/test_coverage.py",
            "python",
        )],
        vec![code_query_symbol(
            "unit-test-coverage",
            "coverage-file",
            "tests/unit/test_coverage.py",
            "UnitTestCoverage",
        )],
        Vec::new(),
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "UnitTestCoverage",
            CodeQueryKind::Symbol,
        ))
        .await
        .expect("symbol query should succeed");

    assert_eq!(hits.len(), 1);
    let lower_hits = store
        .search_code(code_search_request(
            "unittestcoverage",
            CodeQueryKind::Symbol,
        ))
        .await
        .expect("lowercase symbol query should succeed");
    assert!(
        hits[0].score > lower_hits[0].score + 0.7,
        "mixed-case test intent should disable test-path penalty, got {} vs lowercase {}",
        hits[0].score,
        lower_hits[0].score
    );
}

#[tokio::test]
async fn caller_search_preserves_case_for_adapter_intent() {
    let mut call = code_query_call("api-bridge-call", "c-api-file", "db/c.cc");
    call.caller_name = Some("ApiBridge".to_owned());
    call.callee_name = "NewLRUCache".to_owned();
    call.target_hint = Some("NewLRUCache".to_owned());
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file("c-api-file", "db/c.cc", "cpp")],
        Vec::new(),
        vec![call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "ApiBridge NewLRUCache",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    let lower_hits = store
        .search_code(code_search_request(
            "apibridge NewLRUCache",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("lowercase caller query should succeed");
    assert!(
        hits[0].score > lower_hits[0].score + 0.1,
        "mixed-case adapter intent should preserve source-path bonus, got {} vs lowercase {}",
        hits[0].score,
        lower_hits[0].score
    );
}

#[tokio::test]
async fn caller_search_preserves_case_for_test_intent() {
    let mut call = code_query_call("unit-test-call", "cache-file", "src/cache.py");
    call.caller_name = Some("UnitTestCoverage".to_owned());
    call.callee_name = "NewLRUCache".to_owned();
    call.target_hint = Some("NewLRUCache".to_owned());
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file("cache-file", "src/cache.py", "python")],
        Vec::new(),
        vec![call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "UnitTestCoverage NewLRUCache",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    let lower_hits = store
        .search_code(code_search_request(
            "unittestcoverage NewLRUCache",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("lowercase caller query should succeed");
    assert!(
        lower_hits[0].score > hits[0].score + 0.1,
        "mixed-case test intent should disable production source-path bonus, got {} vs lowercase {}",
        hits[0].score,
        lower_hits[0].score
    );
}

#[tokio::test]
async fn caller_search_matches_spaced_compound_identifier_query() {
    let mut call = code_query_call("new-lru-cache-call", "db-file", "db/db_impl.cc");
    call.caller_name = Some("DBImpl::Open".to_owned());
    call.callee_name = "NewLRUCache".to_owned();
    call.target_hint = Some("NewLRUCache".to_owned());
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file("db-file", "db/db_impl.cc", "cpp")],
        Vec::new(),
        vec![call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request("new lru cache", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "db/db_impl.cc");
    assert!(hits[0].excerpt.contains("NewLRUCache"));
}

#[tokio::test]
async fn caller_search_prefers_callers_with_repeated_target_sites() {
    let path = "frontend/dist/js/core/stream.js";
    let mut start = code_query_symbol("start-symbol", "stream-file", path, "startIntentStream");
    start.line_range = code_query_range(100, 180);
    let mut end = code_query_symbol("end-symbol", "stream-file", path, "endStream");
    end.line_range = code_query_range(260, 290);
    let mut attach = code_query_symbol("attach-symbol", "stream-file", path, "attachRunStream");
    attach.line_range = code_query_range(450, 512);

    let mut start_call = code_query_call("start-call", "stream-file", path);
    start_call.caller_symbol_snapshot_id = Some("start-symbol".to_owned());
    start_call.caller_name = Some("startIntentStream".to_owned());
    start_call.callee_symbol_snapshot_id = Some("release-symbol".to_owned());
    start_call.callee_name = "releaseActiveStreamHandle".to_owned();
    start_call.target_hint = Some("releaseActiveStreamHandle".to_owned());
    start_call.line_range = code_query_range(146, 146);

    let mut end_call = code_query_call("end-call", "stream-file", path);
    end_call.caller_symbol_snapshot_id = Some("end-symbol".to_owned());
    end_call.caller_name = Some("endStream".to_owned());
    end_call.callee_symbol_snapshot_id = Some("release-symbol".to_owned());
    end_call.callee_name = "releaseActiveStreamHandle".to_owned();
    end_call.target_hint = Some("releaseActiveStreamHandle".to_owned());
    end_call.line_range = code_query_range(286, 286);

    let mut attach_first = code_query_call("attach-first-call", "stream-file", path);
    attach_first.caller_symbol_snapshot_id = Some("attach-symbol".to_owned());
    attach_first.caller_name = Some("attachRunStream".to_owned());
    attach_first.callee_symbol_snapshot_id = Some("release-symbol".to_owned());
    attach_first.callee_name = "releaseActiveStreamHandle".to_owned();
    attach_first.target_hint = Some("releaseActiveStreamHandle".to_owned());
    attach_first.line_range = code_query_range(478, 478);

    let mut attach_second = code_query_call("attach-second-call", "stream-file", path);
    attach_second.caller_symbol_snapshot_id = Some("attach-symbol".to_owned());
    attach_second.caller_name = Some("attachRunStream".to_owned());
    attach_second.callee_symbol_snapshot_id = Some("release-symbol".to_owned());
    attach_second.callee_name = "releaseActiveStreamHandle".to_owned();
    attach_second.target_hint = Some("releaseActiveStreamHandle".to_owned());
    attach_second.line_range = code_query_range(492, 492);

    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file("stream-file", path, "javascript")],
        vec![start, end, attach],
        vec![start_call, end_call, attach_first, attach_second],
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "releaseActiveStreamHandle",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("caller query should succeed");

    assert!(hits[0].excerpt.contains("attachRunStream"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn caller_search_accepts_scoped_target_hint_prefilter() {
    let mut call = code_query_call("scoped-target-call", "service-file", "src/pkg/service.py");
    call.caller_name = Some("Caller".to_owned());
    call.callee_name = "TargetThing".to_owned();
    call.target_hint = Some("pkg.service.TargetThing".to_owned());
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![code_query_file(
            "service-file",
            "src/pkg/service.py",
            "python",
        )],
        Vec::new(),
        vec![call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request(
            "pkg.service.TargetThing",
            CodeQueryKind::Callers,
        ))
        .await
        .expect("scoped caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/pkg/service.py");
    assert!(hits[0].score >= 5.0, "score was {}", hits[0].score);
}

#[tokio::test]
async fn edge_queries_apply_language_filters_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..520 {
        let file_id = format!("python-noise-file-{index}");
        let path = format!("noise/module_{index}.py");
        files.push(code_query_file(&file_id, &path, "python"));
        let mut call = code_query_call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.callee_name = "TargetThing".to_owned();
        calls.push(call);
    }
    files.push(code_query_file("rust-target-file", "src/lib.rs", "rust"));
    let mut target = code_query_call("zz-rust-target-call", "rust-target-file", "src/lib.rs");
    target.callee_name = "TargetThing".to_owned();
    calls.push(target);
    let store =
        store_with_case_intent_snapshot(code_query_snapshot(files, Vec::new(), calls)).await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should be valid");
    let request = CodeRetrievalRequest::new(
        "TargetThing",
        selector,
        CodeQueryKind::Callers,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid");

    let hits = store
        .search_code(request)
        .await
        .expect("language-filtered caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].language_id, "rust");
}

#[tokio::test]
async fn caller_search_applies_direction_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..520 {
        let file_id = format!("noise-file-{index}");
        let path = format!("noise/caller_{index}.py");
        files.push(code_query_file(&file_id, &path, "python"));
        let mut call = code_query_call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.caller_name = Some("TargetThing".to_owned());
        call.callee_name = "NoiseCallee".to_owned();
        calls.push(call);
    }
    files.push(code_query_file("target-file", "src/service.py", "python"));
    let mut target = code_query_call("zz-target-call", "target-file", "src/service.py");
    target.caller_name = Some("RealCaller".to_owned());
    target.callee_name = "TargetThing".to_owned();
    calls.push(target);
    let store =
        store_with_case_intent_snapshot(code_query_snapshot(files, Vec::new(), calls)).await;

    let hits = store
        .search_code(code_search_request("TargetThing", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/service.py");
    assert!(hits[0].excerpt.contains("TargetThing"));
}

#[tokio::test]
async fn caller_search_demotes_test_call_sites_without_test_intent() {
    let mut test_call = code_query_call(
        "resolved-test-call",
        "filter-test-file",
        "table/filter_block_test.cc",
    );
    test_call.caller_name = Some("TEST_F".to_owned());
    test_call.callee_name = "KeyMayMatch".to_owned();
    test_call.target_hint = Some("KeyMayMatch".to_owned());
    test_call.resolution_state = "resolved".to_owned();
    test_call.confidence_basis_points = 8_000;
    test_call.confidence_tier = "inferred".to_owned();

    let mut production_call =
        code_query_call("ambiguous-production-call", "table-file", "table/table.cc");
    production_call.caller_name = Some("InternalGet".to_owned());
    production_call.callee_name = "KeyMayMatch".to_owned();
    production_call.target_hint = Some("KeyMayMatch".to_owned());
    production_call.confidence_basis_points = 5_000;
    production_call.confidence_tier = "ambiguous".to_owned();

    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![
            code_query_file("filter-test-file", "table/filter_block_test.cc", "cpp"),
            code_query_file("table-file", "table/table.cc", "cpp"),
        ],
        Vec::new(),
        vec![test_call, production_call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request("KeyMayMatch", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, "table/table.cc");
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn caller_search_does_not_promote_repeated_test_sites_without_test_intent() {
    let mut production_call = code_query_call("production-call", "table-file", "table/table.cc");
    production_call.caller_name = Some("InternalGet".to_owned());
    production_call.callee_name = "KeyMayMatch".to_owned();
    production_call.target_hint = Some("KeyMayMatch".to_owned());
    production_call.confidence_basis_points = 5_000;
    production_call.confidence_tier = "ambiguous".to_owned();

    let mut repeated_test_calls = Vec::new();
    for line in [58, 61, 66, 69] {
        let mut call = code_query_call(
            &format!("filter-test-call-{line}"),
            "filter-test-file",
            "table/filter_block_test.cc",
        );
        call.caller_symbol_snapshot_id = Some("filter-test-case".to_owned());
        call.caller_name = Some("TEST_F".to_owned());
        call.callee_symbol_snapshot_id = Some("filter-reader-key-may-match".to_owned());
        call.callee_name = "KeyMayMatch".to_owned();
        call.target_hint = Some("KeyMayMatch".to_owned());
        call.resolution_state = "resolved".to_owned();
        call.confidence_basis_points = 8_000;
        call.confidence_tier = "inferred".to_owned();
        call.line_range = code_query_range(line, line);
        repeated_test_calls.push(call);
    }

    let mut calls = vec![production_call];
    calls.extend(repeated_test_calls);
    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![
            code_query_file("table-file", "table/table.cc", "cpp"),
            code_query_file("filter-test-file", "table/filter_block_test.cc", "cpp"),
        ],
        Vec::new(),
        calls,
    ))
    .await;

    let hits = store
        .search_code(code_search_request("KeyMayMatch", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, "table/table.cc");
    assert!(hits[0].excerpt.contains("InternalGet"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn caller_search_demotes_same_named_wrapper_call_sites() {
    let mut wrapper_call = code_query_call("resolved-wrapper-call", "router-file", "src/router.cc");
    wrapper_call.caller_name = Some("Router::TargetCall".to_owned());
    wrapper_call.callee_name = "TargetCall".to_owned();
    wrapper_call.target_hint = Some("TargetCall".to_owned());
    wrapper_call.resolution_state = "resolved".to_owned();
    wrapper_call.confidence_basis_points = 8_000;
    wrapper_call.confidence_tier = "inferred".to_owned();

    let mut production_call = code_query_call(
        "ambiguous-production-call",
        "service-file",
        "src/service.cc",
    );
    production_call.caller_name = Some("Dispatch".to_owned());
    production_call.callee_name = "TargetCall".to_owned();
    production_call.target_hint = Some("TargetCall".to_owned());
    production_call.confidence_basis_points = 5_000;
    production_call.confidence_tier = "ambiguous".to_owned();

    let store = store_with_case_intent_snapshot(code_query_snapshot(
        vec![
            code_query_file("router-file", "src/router.cc", "cpp"),
            code_query_file("service-file", "src/service.cc", "cpp"),
        ],
        Vec::new(),
        vec![wrapper_call, production_call],
    ))
    .await;

    let hits = store
        .search_code(code_search_request("TargetCall", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, "src/service.cc");
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn callee_search_applies_direction_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..520 {
        let file_id = format!("noise-file-{index}");
        let path = format!("noise/callee_{index}.py");
        files.push(code_query_file(&file_id, &path, "python"));
        let mut call = code_query_call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.caller_name = Some("NoiseCaller".to_owned());
        call.callee_name = "TargetThing".to_owned();
        calls.push(call);
    }
    files.push(code_query_file("target-file", "src/service.py", "python"));
    let mut target = code_query_call("zz-target-call", "target-file", "src/service.py");
    target.caller_name = Some("TargetThing".to_owned());
    target.callee_name = "TargetCallee".to_owned();
    calls.push(target);
    let store =
        store_with_case_intent_snapshot(code_query_snapshot(files, Vec::new(), calls)).await;

    let hits = store
        .search_code(code_search_request("TargetThing", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].path, "src/service.py");
    assert!(hits[0].excerpt.contains("TargetCallee"));
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
        chunks: Vec::new(),
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
