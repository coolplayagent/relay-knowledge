use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeFileDiagnostic, CodeImportRecord, CodeIndexSnapshot, CodeParseStatus,
        CodeQueryKind, CodeRepositorySelector, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:fixture:commit:tree";

#[path = "code_query_accuracy_tests/call_chunk_fixtures.rs"]
mod call_chunk_fixtures;
#[path = "code_query_accuracy_tests/call_chunk_tests.rs"]
mod call_chunk_tests;
#[path = "code_query_accuracy_tests/definition_fixtures.rs"]
mod definition_fixtures;
#[path = "code_query_accuracy_tests/definition_tests.rs"]
mod definition_tests;
#[path = "code_query_accuracy_tests/import_fixtures.rs"]
mod import_fixtures;
#[path = "code_query_accuracy_tests/import_tests.rs"]
mod import_tests;
#[path = "code_query_accuracy_tests/ranking_fixtures.rs"]
mod ranking_fixtures;
#[path = "code_query_accuracy_tests/scope_fixtures.rs"]
mod scope_fixtures;
#[path = "code_query_accuracy_tests/scope_tests.rs"]
mod scope_tests;

use call_chunk_fixtures::*;
use definition_fixtures::*;
use import_fixtures::*;
use ranking_fixtures::*;
use scope_fixtures::*;

fn file(
    file_id: &str,
    path: &str,
    language_id: &str,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status,
        degraded_reason,
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
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "cpp".to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range: range(110, 124),
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
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

fn import(
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    resolution_state: &str,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: target_hint.map(str::to_owned),
        resolution_state: resolution_state.to_owned(),
        confidence_basis_points: if resolution_state == "resolved" {
            8_000
        } else {
            2_500
        },
        confidence_tier: if resolution_state == "resolved" {
            "inferred".to_owned()
        } else {
            "ambiguous".to_owned()
        },
        line_range: range(1, 1),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    store_with_repository_snapshot_and_filters(snapshot, Vec::new(), Vec::new()).await
}

async fn store_with_repository_snapshot_and_filters(
    mut snapshot: CodeIndexSnapshot,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        "/tmp/repo",
        path_filters.clone(),
        language_filters.clone(),
    )
    .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    snapshot.path_filters = path_filters;
    snapshot.language_filters = language_filters;
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
