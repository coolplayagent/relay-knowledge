use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord,
    },
    storage::{SqliteGraphStore, code::CodeRepositoryStore},
};

const TEST_SOURCE_SCOPE: &str = "code:test:line-context";

#[tokio::test]
async fn class_definition_hits_include_bounded_previous_symbol_context() {
    let store = store_with_repository_snapshot(snapshot_with_connector_service_context()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "ConnectorService",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].excerpt, "class ConnectorService:");
    assert_eq!(hits[0].line_range, range(134, 779));
}

#[tokio::test]
async fn caller_hits_return_owning_symbol_context_when_edge_is_resolved() {
    let store = store_with_repository_snapshot(snapshot_with_connector_service_context()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_summary",
                selector,
                CodeQueryKind::Callers,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].excerpt, "list_connectors calls _summary");
    assert_eq!(hits[0].line_range, range(177, 194));
}

fn snapshot_with_connector_service_context() -> CodeIndexSnapshot {
    let mut protocol = symbol(
        "runtime-tool-service-like",
        "connector-service-file",
        "src/relay_teams/connector/service.py",
        "RuntimeToolServiceLike",
    );
    protocol.kind = "class".to_owned();
    protocol.language_id = "python".to_owned();
    protocol.signature = "class RuntimeToolServiceLike(Protocol):".to_owned();
    protocol.line_range = range(134, 136);

    let mut service = symbol(
        "connector-service",
        "connector-service-file",
        "src/relay_teams/connector/service.py",
        "ConnectorService",
    );
    service.kind = "class".to_owned();
    service.language_id = "python".to_owned();
    service.signature = "class ConnectorService:".to_owned();
    service.line_range = range(150, 779);

    let mut init = symbol(
        "connector-service-init",
        "connector-service-file",
        "src/relay_teams/connector/service.py",
        "__init__",
    );
    init.kind = "method".to_owned();
    init.language_id = "python".to_owned();
    init.qualified_name = "ConnectorService.__init__".to_owned();
    init.canonical_symbol_id = init.qualified_name.clone();
    init.signature = "def __init__(self) -> None:".to_owned();
    init.line_range = range(151, 176);

    let mut list_connectors = symbol(
        "connector-service-list",
        "connector-service-file",
        "src/relay_teams/connector/service.py",
        "list_connectors",
    );
    list_connectors.kind = "method".to_owned();
    list_connectors.language_id = "python".to_owned();
    list_connectors.qualified_name = "ConnectorService.list_connectors".to_owned();
    list_connectors.canonical_symbol_id = list_connectors.qualified_name.clone();
    list_connectors.signature =
        "async def list_connectors(self) -> ConnectorListResponse:".to_owned();
    list_connectors.line_range = range(178, 194);

    let mut call = call(
        "connector-service-list-summary",
        "connector-service-file",
        "src/relay_teams/connector/service.py",
    );
    call.caller_symbol_snapshot_id = Some("connector-service-list".to_owned());
    call.caller_name = Some("list_connectors".to_owned());
    call.callee_name = "_summary".to_owned();
    call.target_hint = Some("_summary".to_owned());
    call.resolution_state = "resolved".to_owned();
    call.confidence_basis_points = 8_000;
    call.confidence_tier = "inferred".to_owned();
    call.line_range = range(194, 194);

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
            "connector-service-file",
            "src/relay_teams/connector/service.py",
        )],
        symbols: vec![protocol, service, init, list_connectors],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "python".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 800,
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
        canonical_symbol_id: name.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "python".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("def {name}():"),
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

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
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
