use super::*;

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
