use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:call-ranking:commit:tree";

#[tokio::test]
async fn callees_rank_receiver_qualified_member_call_sites() {
    let path = "packages/llm/src/route/client.ts";
    let mut caller_symbol = symbol("stream-with-symbol", "client-file", path, "streamWith");
    caller_symbol.line_range = range(40, 60);
    let caller_chunk = chunk(
        "stream-with-chunk",
        "client-file",
        path,
        "streamWith = (input) => {\n  const prepared = prepareRequest(input)\n  return ToolRuntime.stream({ ...input, stream: prepared })\n}",
        Some("stream-with-symbol"),
        range(44, 49),
    );

    let mut member_call = call("member-call", "client-file", path);
    member_call.caller_symbol_snapshot_id = Some("stream-with-symbol".to_owned());
    member_call.caller_name = Some("streamWith".to_owned());
    member_call.callee_name = "stream".to_owned();
    member_call.target_hint = Some("stream".to_owned());
    member_call.confidence_basis_points = 5_000;
    member_call.confidence_tier = "ambiguous".to_owned();
    member_call.line_range = range(47, 47);

    let mut helper_call = call("helper-call", "client-file", path);
    helper_call.caller_symbol_snapshot_id = Some("stream-with-symbol".to_owned());
    helper_call.caller_name = Some("streamWith".to_owned());
    helper_call.callee_name = "prepareRequest".to_owned();
    helper_call.target_hint = Some("prepareRequest".to_owned());
    helper_call.resolution_state = "resolved".to_owned();
    helper_call.confidence_basis_points = 8_000;
    helper_call.confidence_tier = "inferred".to_owned();
    helper_call.line_range = range(46, 46);

    let store = store_with_snapshot(CodeIndexSnapshot {
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
        files: vec![file("client-file", path, "typescript")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![member_call, helper_call],
        chunks: vec![caller_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("streamWith", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert!(hits[0].excerpt.contains("ToolRuntime.stream"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn callees_apply_direction_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..520 {
        let file_id = format!("noise-file-{index}");
        let path = format!("noise/callee_{index}.py");
        files.push(file(&file_id, &path, "python"));
        let mut call = call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.caller_name = Some("NoiseCaller".to_owned());
        call.callee_name = "TargetThing".to_owned();
        calls.push(call);
    }
    files.push(file("target-file", "src/service.py", "python"));
    let mut target = call("zz-target-call", "target-file", "src/service.py");
    target.caller_name = Some("TargetThing".to_owned());
    target.callee_name = "TargetCallee".to_owned();
    calls.push(target);
    let store = store_with_snapshot(CodeIndexSnapshot {
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
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("TargetThing", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].path, "src/service.py");
    assert!(hits[0].excerpt.contains("TargetCallee"));
}

#[test]
fn callee_member_context_bonus_requires_member_call_shape() {
    let callees = request("OwnerTarget", CodeQueryKind::Callees);
    let callers = request("OwnerTarget", CodeQueryKind::Callers);

    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime.stream({ input })"),
            "stream",
            &callees,
        ),
        0.45
    );
    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime::stream(input)"),
            "stream",
            &callees,
        ),
        0.45
    );
    assert_eq!(
        callee_member_context_bonus(4.0, Some("stream: streamRequest"), "stream", &callees,),
        0.0
    );
    assert_eq!(
        callee_member_context_bonus(4.0, Some("stream(input)"), "stream", &callees),
        0.0
    );
    assert_eq!(
        callee_member_context_bonus(
            4.0,
            Some("return ToolRuntime.stream({ input })"),
            "stream",
            &callers,
        ),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 80,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("const {name} = () => {{}}"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn call(call_id: &str, file_id: &str, path: &str) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
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
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range,
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
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
