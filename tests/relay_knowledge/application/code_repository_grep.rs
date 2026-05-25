use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeChunkRecord, CodeFileFingerprint, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeImpactRequest, CodeIndexCheckpoint, CodeIndexMode, CodeIndexRequest, CodeIndexSummary,
        CodeIndexTaskRecord, CodeQueryKind, CodeReferenceRecord, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, CodeScopeRetentionSummary, CodeSymbolRecord, CommitReceipt,
        FreshnessPolicy, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RepositoryCodeRange, RetrievalHit,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeImpactChanges, CodeIndexTaskClaimRequest,
        CodeIndexTaskCompletion, CodeIndexTaskFailure, CodeIndexTaskSeed,
        CodeReferenceSearchRequest, CodeRepositoryStore, CodeScopeRetentionRequest,
        CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest, GraphStore, IndexStore,
        KnowledgeStore, MutationLogEntry, MutationLogStore, SqliteGraphStore, StorageError,
        StorageFuture,
    },
};

#[tokio::test]
async fn reference_query_uses_ripgrep_text_fallback_for_comment_reference() {
    if Command::new("rg").arg("--version").output().is_err() {
        return;
    }
    let repo = FixtureRepo::create("code-ripgrep-reference");
    repo.write(
        "include/macros.h",
        "#ifndef RK_MACROS_H\n#define RK_MACROS_H\n#define RK_TRACE_VALUE(value) ((value) + 17)\n#endif\n",
    );
    repo.write(
        "src/main.c",
        "#include \"../include/macros.h\"\n// RK_TRACE_NOTE documents fallback-only macro text.\nint read_value(int input) {\n    return RK_TRACE_VALUE(input);\n}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "macro reference"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["include".to_owned(), "src".to_owned()],
                language_filters: vec!["c".to_owned()],
            },
            context("register-ripgrep-reference"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-ripgrep-reference"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "RK_TRACE_NOTE",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec!["src/main.c".to_owned()],
                    vec!["c".to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::References,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-ripgrep-reference"),
        )
        .await
        .expect("query should succeed");

    let hit = response
        .results
        .iter()
        .find(|hit| {
            hit.excerpt
                .contains("RK_TRACE_NOTE documents fallback-only macro text")
        })
        .expect("comment reference should be recovered");
    assert!(hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical));
    assert!(
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
    assert!(hit.edge_confidence_basis_points.is_none());
    assert!(hit.edge_confidence_tier.is_none());
}

#[tokio::test]
async fn ripgrep_fallback_uses_query_candidates_before_scope_file_budget() {
    if Command::new("rg").arg("--version").output().is_err() {
        return;
    }
    let repo = FixtureRepo::create("code-ripgrep-query-candidate-budget");
    for index in 0..300 {
        repo.write(
            &format!("src/noise_{index:03}.c"),
            &format!("int noise_{index:03}(void) {{ return {index}; }}\n"),
        );
    }
    repo.write(
        "zzz/late_target.c",
        "// RK_LATE_BUDGET_NOTE must stay reachable after broad candidate selection.\nint late_budget_target(void) { return 7; }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "late grep target"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned(), "zzz".to_owned()],
                language_filters: vec!["c".to_owned()],
            },
            context("register-ripgrep-query-candidates"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-ripgrep-query-candidates"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "RK_LATE_BUDGET_NOTE",
                CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), vec!["c".to_owned()])
                    .expect("selector should validate"),
                CodeQueryKind::References,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-ripgrep-query-candidates"),
        )
        .await
        .expect("query should succeed");

    assert!(response.degraded_reason.is_none());
    assert!(response.results.iter().any(|hit| {
        hit.path == "zzz/late_target.c"
            && hit
                .excerpt
                .contains("RK_LATE_BUDGET_NOTE must stay reachable")
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
    }));
}

#[tokio::test]
async fn definition_query_line_scans_when_ripgrep_omits_long_lines() {
    let repo = FixtureRepo::create("code-ripgrep-long-line-definition");
    let filler = "x".repeat(5000);
    repo.write(
        "docs/api.txt",
        &format!("int rk_long_line_definition(void); // {filler}\n"),
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "long definition line"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "long-line-fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-long-line-definition"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("long-line-fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-long-line-definition"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "rk_long_line_definition",
                CodeRepositorySelector::new(
                    "long-line-fixture",
                    "HEAD",
                    vec!["docs/api.txt".to_owned()],
                    vec!["unknown".to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-long-line-definition"),
        )
        .await
        .expect("query should succeed");

    let hit = response
        .results
        .iter()
        .find(|hit| {
            hit.path == "docs/api.txt"
                && hit.excerpt.contains("int rk_long_line_definition(void);")
                && hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::Definition)
                && hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
        })
        .expect("long definition line should be recovered by line scanner");
    assert!(hit.edge_confidence_basis_points.is_none());
    assert!(hit.edge_confidence_tier.is_none());
}

#[tokio::test]
async fn query_degrades_when_candidate_path_lookup_is_unavailable() {
    let repo = FixtureRepo::create("code-ripgrep-candidate-path-unavailable");
    repo.write(
        "src/lib.c",
        "int rk_unavailable_candidate(void) { return 1; }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "candidate path unavailable"]);
    let commit = repo.git_stdout(["rev-parse", "HEAD"]);
    let store = Arc::new(CandidatePathUnavailableStore {
        status: repository_status(&repo.path, &commit),
    });
    let service = service_with_store(store).await;

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "RK_UNAVAILABLE_REFERENCE",
                selector("fixture", "HEAD"),
                CodeQueryKind::References,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-candidate-path-unavailable"),
        )
        .await
        .expect("query should keep structured results when grep path lookup is unavailable");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].path, "src/lib.c");
    assert!(
        response
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("ripgrep candidate path lookup unavailable")),
        "unexpected degraded reason: {:?}",
        response.degraded_reason
    );
}

#[tokio::test]
async fn import_query_uses_grep_fallback_for_unindexed_external_dependency() {
    if Command::new("rg").arg("--version").output().is_err() {
        return;
    }
    let repo = FixtureRepo::create("code-ripgrep-external-import");
    repo.write(
        "src/component.tsx",
        r#"
import React from "react";

export function Panel({ value }: { value: string }) {
    const [state, setState] = React.useState(value);
    React.useEffect(() => setState(value.trim()), [value]);
    return <section>{state}</section>;
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "external import"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: vec!["tsx".to_owned()],
            },
            context("register-external-import"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-external-import"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "react",
                CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), vec!["tsx".to_owned()])
                    .expect("selector should validate"),
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-external-import"),
        )
        .await
        .expect("query should succeed");

    assert!(
        response
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("external dependency import is not indexed")),
        "unexpected degraded reason: {:?}",
        response.degraded_reason
    );
    assert!(
        response.results.iter().any(|hit| {
            hit.path == "src/component.tsx"
                && hit.edge_kind.as_deref() == Some("import")
                && hit.edge_resolution_state.as_deref() == Some("unresolved")
        }),
        "expected unresolved import graph evidence: {:?}",
        response.results
    );
    assert!(
        response.results.iter().any(|hit| {
            hit.path == "src/component.tsx"
                && hit.excerpt.contains("react")
                && hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
        }),
        "expected current-repository text fallback evidence: {:?}",
        response.results
    );
}

fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    RelayKnowledgeService::with_store(runtime, store)
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}

fn repository_status(root: &std::path::Path, commit: &str) -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: root.display().to_string(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some(commit.to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    }
}

struct CandidatePathUnavailableStore {
    status: CodeRepositoryStatus,
}

impl GraphStore for CandidatePathUnavailableStore {
    fn commit_mutation_batch(
        &self,
        _batch: GraphMutationBatch,
    ) -> StorageFuture<'_, CommitReceipt> {
        unsupported("candidate fixture does not commit graph batches")
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        unsupported("candidate fixture does not inspect graphs")
    }

    fn search(&self, _request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        unsupported("candidate fixture does not search graphs")
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async { Ok(GraphVersion::new(1)) })
    }
}

impl MutationLogStore for CandidatePathUnavailableStore {
    fn read_after(
        &self,
        _graph_version: GraphVersion,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        unsupported("candidate fixture does not read mutation logs")
    }
}

impl IndexStore for CandidatePathUnavailableStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        unsupported("candidate fixture does not read index statuses")
    }

    fn mark_refresh_complete(
        &self,
        _kind: IndexKind,
        _graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        unsupported("candidate fixture does not mark refresh complete")
    }
}

impl CodeGraphStore for CandidatePathUnavailableStore {
    fn commit_code_graph_batch(
        &self,
        _batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        unsupported("candidate fixture does not commit code graph batches")
    }

    fn search_code_symbols(
        &self,
        _request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        unsupported("candidate fixture does not search code symbols")
    }

    fn search_code_references(
        &self,
        _request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        unsupported("candidate fixture does not search code references")
    }

    fn search_code_chunks(
        &self,
        _request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        unsupported("candidate fixture does not search code chunks")
    }
}

macro_rules! unsupported_code_repository_method {
    ($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        fn $name(&self, $($arg: $ty),*) -> StorageFuture<'_, $ret> {
            $(let _ = $arg;)*
            unsupported("candidate fixture method is unavailable")
        }
    };
}

impl CodeRepositoryStore for CandidatePathUnavailableStore {
    unsupported_code_repository_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);

    fn code_repository_status(
        &self,
        _repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        let status = self.status.clone();
        Box::pin(async move { Ok(Some(status)) })
    }

    fn code_repository_scope_status(
        &self,
        _repository: String,
        _resolved_commit_sha: String,
        _path_filters: Vec<String>,
        _language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        let status = self.status.clone();
        Box::pin(async move { Ok(Some(status)) })
    }

    unsupported_code_repository_method!(queue_code_index_task(task: CodeIndexTaskSeed) -> CodeIndexTaskRecord);
    unsupported_code_repository_method!(claim_code_index_task(request: CodeIndexTaskClaimRequest) -> Option<CodeIndexTaskRecord>);
    unsupported_code_repository_method!(complete_code_index_task(request: CodeIndexTaskCompletion) -> CodeIndexTaskRecord);
    unsupported_code_repository_method!(fail_code_index_task(request: CodeIndexTaskFailure) -> CodeIndexTaskRecord);
    unsupported_code_repository_method!(code_index_task(task_id: String) -> Option<CodeIndexTaskRecord>);
    unsupported_code_repository_method!(active_code_index_task(repository_id: String) -> Option<CodeIndexTaskRecord>);
    unsupported_code_repository_method!(code_index_checkpoint(source_scope: String) -> Option<CodeIndexCheckpoint>);
    unsupported_code_repository_method!(code_scope_retention(repository_id: String) -> CodeScopeRetentionSummary);
    unsupported_code_repository_method!(prune_code_repository_scopes(request: CodeScopeRetentionRequest) -> CodeScopeRetentionSummary);
    unsupported_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_repository_method!(apply_code_index_snapshot(snapshot: relay_knowledge::domain::CodeIndexSnapshot) -> CodeIndexSummary);

    fn search_code(
        &self,
        _request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let status = self.status.clone();
        Box::pin(async move { Ok(vec![structured_hit(&status)]) })
    }

    unsupported_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}

fn structured_hit(status: &CodeRepositoryStatus) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: status.repository_id.clone(),
        scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
        resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
        tree_hash: status.tree_hash.clone().unwrap_or_default(),
        path: "src/lib.c".to_owned(),
        language_id: "c".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 12 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "structured code result".to_owned(),
    }
}

fn unsupported<T: Send + 'static>(message: &'static str) -> StorageFuture<'static, T> {
    Box::pin(async move { Err(StorageError::InvalidInput(message.to_owned())) })
}

struct FixtureRepo {
    path: PathBuf,
}

impl FixtureRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("repo directory should be created");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_stdout<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout should be utf8")
            .trim()
            .to_owned()
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
