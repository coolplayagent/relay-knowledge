use super::*;
use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::SqliteGraphStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:import-foundational-ranking:commit:tree";

#[tokio::test]
async fn module_import_queries_rank_production_importer_before_test_importer() {
    let service_path = "src/relay_teams/connector/service.py";
    let test_path = "tests/unit_tests/connector/test_w3_service.py";
    let store = store_with_snapshot(
        vec![
            file("service-file", service_path, "python"),
            file("test-file", test_path, "python"),
        ],
        vec![
            import(
                "service-models",
                "service-file",
                service_path,
                "from relay_teams.connector.models import (ConnectorCategory, ConnectorListResponse)",
            ),
            import(
                "test-models",
                "test-file",
                test_path,
                "from relay_teams.connector.models import ConnectorCategory",
            ),
        ],
        vec![
            chunk(
                "service-chunk",
                "service-file",
                service_path,
                "return ConnectorListResponse(summary=summary, items=items)",
                "python",
            ),
            chunk(
                "test-chunk",
                "test-file",
                test_path,
                "assert ConnectorCategory('w3')",
                "python",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request(
            "relay_teams.connector.models",
            CodeQueryKind::Imports,
        ))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, service_path);
}

#[tokio::test]
async fn extensionless_relative_import_queries_rank_early_direct_import_site() {
    let matching_path = "packages/http-recorder/src/matching.ts";
    let redactor_path = "packages/http-recorder/src/redactor.ts";
    let cassette_path = "packages/http-recorder/src/cassette.ts";
    let mut matching_import = import(
        "matching-redaction",
        "matching-file",
        matching_path,
        "import { REDACTED, secretFindings } from \"./redaction\"",
    );
    matching_import.line_range = range(2, 2);
    let mut redactor_import = import(
        "redactor-redaction",
        "redactor-file",
        redactor_path,
        "import { redactHeaders, redactUrl } from \"./redaction\"",
    );
    redactor_import.line_range = range(3, 3);
    let mut cassette_import = import(
        "cassette-redaction",
        "cassette-file",
        cassette_path,
        "import { secretFindings, SecretFindingSchema, type SecretFinding } from \"./redaction\"",
    );
    cassette_import.line_range = range(4, 4);
    let store = store_with_snapshot(
        vec![
            file("matching-file", matching_path, "typescript"),
            file("redactor-file", redactor_path, "typescript"),
            file("cassette-file", cassette_path, "typescript"),
        ],
        vec![redactor_import, cassette_import, matching_import],
        vec![
            chunk(
                "matching-chunk",
                "matching-file",
                matching_path,
                "if (secretFindings(value).length > 0) return JSON.stringify(REDACTED)",
                "typescript",
            ),
            chunk(
                "redactor-chunk",
                "redactor-file",
                redactor_path,
                "headers: redactHeaders(input.headers)\nurl: redactUrl(input.url)",
                "typescript",
            ),
            chunk(
                "cassette-chunk",
                "cassette-file",
                cassette_path,
                "entry.findings.push(...secretFindings(interaction))",
                "typescript",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request("./redaction", CodeQueryKind::Imports))
        .await
        .expect("relative import query should succeed");

    assert_eq!(hits[0].path, matching_path);
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
        line_count: 20,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn import(import_id: &str, file_id: &str, path: &str, module: &str) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    language_id: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range: range(1, 20),
        symbol_snapshot_id: None,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(
    files: Vec<RepositoryCodeFileRecord>,
    imports: Vec<CodeImportRecord>,
    chunks: Vec<RepositoryCodeChunkRecord>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(CodeIndexSnapshot {
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
            imports,
            calls: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks,
            diagnostics: Vec::new(),
        })
        .await
        .expect("snapshot should apply");
    store
}
