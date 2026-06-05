use super::*;
use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
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
    let redaction_path = "packages/http-recorder/src/redaction.ts";
    let mut matching_import = import_with_target(
        "matching-redaction",
        "matching-file",
        matching_path,
        "import { REDACTED, secretFindings } from \"./redaction\"",
        redaction_path,
    );
    matching_import.line_range = range(2, 2);
    let mut redactor_import = import_with_target(
        "redactor-redaction",
        "redactor-file",
        redactor_path,
        "import { redactHeaders, redactUrl } from \"./redaction\"",
        redaction_path,
    );
    redactor_import.line_range = range(3, 3);
    let mut cassette_import = import_with_target(
        "cassette-redaction",
        "cassette-file",
        cassette_path,
        "import { secretFindings, SecretFindingSchema, type SecretFinding } from \"./redaction\"",
        redaction_path,
    );
    cassette_import.line_range = range(4, 4);
    let store = store_with_symbols(
        vec![
            file("matching-file", matching_path, "typescript"),
            file("redactor-file", redactor_path, "typescript"),
            file("cassette-file", cassette_path, "typescript"),
            file("redaction-file", redaction_path, "typescript"),
        ],
        vec![redactor_import, cassette_import, matching_import],
        vec![
            symbol(
                "redaction-helper-one",
                "redaction-file",
                redaction_path,
                "envSecrets",
            ),
            symbol(
                "redaction-helper-two",
                "redaction-file",
                redaction_path,
                "pathFor",
            ),
            symbol(
                "redaction-helper-three",
                "redaction-file",
                redaction_path,
                "stringEntries",
            ),
            symbol(
                "redaction-helper-four",
                "redaction-file",
                redaction_path,
                "redactionSet",
            ),
        ],
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

fn import_with_target(
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
    target_hint: &str,
) -> CodeImportRecord {
    let mut record = import(import_id, file_id, path, module);
    record.target_hint = Some(target_hint.to_owned());
    record
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
        signature: format!("export const {name} = () => undefined"),
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
    store_with_symbols(files, imports, Vec::new(), chunks).await
}

async fn store_with_symbols(
    files: Vec<RepositoryCodeFileRecord>,
    imports: Vec<CodeImportRecord>,
    symbols: Vec<RepositoryCodeSymbolRecord>,
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
            symbols,
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
