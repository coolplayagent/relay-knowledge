use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, ErrorKind, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalLayer, CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn indexes_tree_sitter_repository_and_queries_code_graph() {
    let repo = FixtureRepo::create("code-retrieval");
    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    3
}

pub enum Color {
    Red,
    Blue,
}
"#,
    );
    repo.write(
        "src/main.rs",
        r#"
use crate::retry_policy;

fn run_worker() {
    retry_policy();
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let first_commit = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register"),
        )
        .await
        .expect("repository should register");
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index"),
        )
        .await
        .expect("repository should index");

    assert_eq!(indexed.summary.indexed_file_count, 2);
    assert!(indexed.summary.symbol_count >= 2);

    let definitions = query(&service, "retry_policy", CodeQueryKind::Definition).await;
    let doc_comment = query(&service, "budget", CodeQueryKind::Definition).await;
    let enum_member_definition = query(&service, "Red", CodeQueryKind::Definition).await;
    let enum_member_symbol = query(&service, "Color.Red", CodeQueryKind::Symbol).await;
    let references = query(&service, "retry_policy", CodeQueryKind::References).await;
    let imports = query(&service, "crate::retry_policy", CodeQueryKind::Imports).await;

    assert!(
        definitions
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(
        doc_comment
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert_eq!(doc_comment.scope.resolved_commit_sha, first_commit.as_str());
    assert_eq!(doc_comment.scope.path_filters, ["src"]);
    assert!(
        doc_comment
            .scope
            .index_versions
            .iter()
            .any(|version| version.starts_with("code:"))
    );
    assert!(
        enum_member_definition.results.iter().any(|hit| {
            hit.path == "src/lib.rs"
                && hit
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.contains("Color.Red"))
                && hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
                && !hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
        }),
        "enum member definition should come from the symbol index: {:?}",
        enum_member_definition.results
    );
    assert!(
        enum_member_symbol.results.iter().any(|hit| {
            hit.path == "src/lib.rs"
                && hit
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.contains("Color.Red"))
        }),
        "scoped enum member symbol query should match the owner-qualified identity: {:?}",
        enum_member_symbol.results
    );
    assert!(
        references
            .results
            .iter()
            .any(|hit| hit.path == "src/main.rs")
    );
    assert!(imports.results.iter().any(|hit| hit.path == "src/main.rs"));

    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    5
}

pub fn retry_policy_v2() -> u32 {
    retry_policy()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update policy"]);
    let updated = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(first_commit, "HEAD")
                    .expect("incremental mode should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("update"),
        )
        .await
        .expect("incremental update should index");

    assert_eq!(updated.summary.changed_path_count, 1);
    assert!(updated.summary.indexed_file_count >= 2);

    let v2 = query(&service, "retry_policy_v2", CodeQueryKind::Definition).await;
    assert!(v2.results.iter().any(|hit| hit.path == "src/lib.rs"));

    let impact = service
        .impact_code_repository(
            CodeImpactRequest::new(selector("fixture", "HEAD"), "HEAD~1", "HEAD", 10)
                .expect("impact request should validate"),
            context("impact"),
        )
        .await
        .expect("impact should succeed");

    assert!(
        impact
            .path_groups
            .in_scope_changed_paths
            .iter()
            .any(|path| path == "src/lib.rs")
    );
    assert!(impact.results.iter().any(|hit| hit.path == "src/lib.rs"));

    repo.write("src/late.rs", "pub fn late_change() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "late change"]);
    let stale_head_error = service
        .impact_code_repository(
            CodeImpactRequest::new(
                selector("fixture", &updated.summary.resolved_commit_sha),
                &updated.summary.resolved_commit_sha,
                "HEAD",
                10,
            )
            .expect("impact request should validate"),
            context("impact-stale-head"),
        )
        .await
        .expect_err("impact head must match indexed snapshot");

    assert!(stale_head_error.message.contains("no index for ref"));
}

#[tokio::test]
async fn indexes_sql_schema_files_into_code_graph() {
    let repo = FixtureRepo::create("code-sql-schema");
    repo.write(
        "src/schema.sql",
        r#"
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    organization_id INTEGER REFERENCES organizations(id)
);

CREATE VIEW active_users AS
SELECT id FROM users;
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "schema"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-sql-schema").await;
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-sql-schema"),
        )
        .await
        .expect("SQL repository should index");

    assert_eq!(indexed.summary.indexed_file_count, 1);
    assert!(indexed.summary.symbol_count >= 2);
    assert!(indexed.summary.reference_count >= 2);

    let table = query(&service, "users", CodeQueryKind::Definition).await;
    let references = query(&service, "users", CodeQueryKind::References).await;

    assert!(
        table.results.iter().any(|hit| {
            hit.path == "src/schema.sql"
                && hit
                    .canonical_symbol_id
                    .as_deref()
                    .is_some_and(|id| id.contains("users"))
        }),
        "SQL table definition should be queryable: {:?}",
        table.results
    );
    assert!(
        references.results.iter().any(|hit| {
            hit.path == "src/schema.sql" && hit.edge_kind.as_deref() == Some("reference")
        }),
        "SQL object references should be queryable: {:?}",
        references.results
    );
}

#[tokio::test]
async fn register_rejects_language_filters_to_preserve_full_language_surface() {
    let repo = FixtureRepo::create("code-register-language-rejected");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    let error = service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: vec!["rust".to_owned()],
            },
            context("register-language-rejected"),
        )
        .await
        .expect_err("registration language filters should be rejected");

    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(
        error
            .message
            .contains("registration language filters are not supported")
    );
    assert!(error.message.contains("query-time --language"));
}

#[tokio::test]
async fn incremental_index_uses_persisted_base_scope_when_head_is_active() {
    let repo = FixtureRepo::create("code-incremental-base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-incremental-base"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-incremental-base"),
        )
        .await
        .expect("initial index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update to one"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial.clone(), "HEAD")
                    .expect("incremental mode should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-current-base"),
        )
        .await
        .expect("first incremental index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "return to zero"]);
    let updated_from_persisted_base = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial, "HEAD")
                    .expect("incremental mode should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-persisted-base"),
        )
        .await
        .expect("persisted base scope should seed incremental update");

    assert_eq!(updated_from_persisted_base.summary.changed_path_count, 1);
    assert_eq!(
        updated_from_persisted_base.summary.progress.blob_read_count,
        1
    );
    assert!(
        query(&service, "value", CodeQueryKind::Definition)
            .await
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
}

#[tokio::test]
async fn duplicate_root_registration_preserves_existing_aliases() {
    let repo = FixtureRepo::create("code-aliases");
    repo.write("src/lib.rs", "pub fn aliased() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-primary-alias").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-primary-alias"),
        )
        .await
        .expect("repository should index under primary alias");
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture-web".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-secondary-alias"),
        )
        .await
        .expect("same root should accept an additional alias");

    let primary = service
        .code_repository_status(selector("fixture", "HEAD"), context("status-primary-alias"))
        .await
        .expect("primary alias should still resolve");
    let secondary = service
        .code_repository_status(
            selector("fixture-web", "HEAD"),
            context("status-secondary-alias"),
        )
        .await
        .expect("secondary alias should resolve");

    assert_eq!(primary.status.repository_id, secondary.status.repository_id);
    assert_eq!(primary.status.alias, "fixture");
    assert_eq!(secondary.status.alias, "fixture-web");
    assert_eq!(primary.status.indexed_file_count, 1);
    assert_eq!(secondary.status.indexed_file_count, 1);
}

#[tokio::test]
async fn alias_collision_across_repositories_is_rejected() {
    let first = FixtureRepo::create("code-alias-collision-first");
    first.write("src/lib.rs", "pub fn first_alias() -> u32 { 1 }\n");
    first.git(["add", "."]);
    first.git(["commit", "-m", "initial"]);
    let second = FixtureRepo::create("code-alias-collision-second");
    second.write("src/lib.rs", "pub fn second_alias() -> u32 { 2 }\n");
    second.git(["add", "."]);
    second.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: first.path.display().to_string(),
                alias: "shared".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-shared-first"),
        )
        .await
        .expect("first repository should register");
    let error = service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: second.path.display().to_string(),
                alias: "shared".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-shared-second"),
        )
        .await
        .expect_err("same alias should not point at a different repository id");

    assert!(error.message.contains("already registered"));
}

#[tokio::test]
async fn health_graph_code_counters_include_repository_totals() {
    let repo = FixtureRepo::create("code-health-totals");
    repo.write("src/lib.rs", "pub fn health_total() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-health-totals").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-health-totals"),
        )
        .await
        .expect("repository should index");

    let health = service
        .health(context("health-code-totals"))
        .await
        .expect("health should report code totals");

    assert_eq!(health.repository_code_totals.indexed_file_count, 1);
    assert_eq!(
        health.graph.code_file_count,
        health.repository_code_totals.indexed_file_count
    );
    assert_eq!(
        health.graph.code_symbol_count,
        health.repository_code_totals.symbol_count
    );
    assert_eq!(health.graph.code_parse_status_counts.parsed, 1);
}

#[tokio::test]
async fn full_index_reuses_fresh_matching_scope_without_rebuilding() {
    let repo = FixtureRepo::create("code-full-noop");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-full-noop").await;
    let first = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-full-noop-first"),
        )
        .await
        .expect("initial full index should succeed");
    let second = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-full-noop-second"),
        )
        .await
        .expect("fresh full index should reuse scope");

    assert_eq!(second.summary.source_scope, first.summary.source_scope);
    assert_eq!(second.summary.changed_path_count, 0);
    assert_eq!(second.summary.skipped_unchanged_count, 1);
    assert_eq!(second.summary.progress.blob_read_count, 0);
    assert_eq!(second.summary.progress.parsed_file_count, 0);
    assert_eq!(second.summary.progress.sqlite_write_count, 0);
}

#[tokio::test]
async fn repository_report_does_not_run_latency_samples_by_default() {
    let repo = FixtureRepo::create("code-report-fast");
    repo.write("src/lib.rs", "pub fn report_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-report-fast").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-report-fast"),
        )
        .await
        .expect("repository should index");
    let report = service
        .code_repository_report(selector("fixture", "HEAD"), context("report-fast"))
        .await
        .expect("report should succeed");

    assert!(report.report.latency_samples.is_empty());
    assert!(
        report
            .report
            .representative_queries
            .iter()
            .any(|query| query == "report_policy")
    );
}

#[tokio::test]
async fn callee_query_does_not_reuse_caller_symbol_identity_for_unresolved_edges() {
    let repo = FixtureRepo::create("code-unresolved-callee");
    repo.write(
        "src/lib.rs",
        r#"
pub fn caller_missing() {
    missing_dependency();
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-unresolved-callee"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-unresolved-callee"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "caller_missing",
                selector("fixture", "HEAD"),
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-unresolved-callee"),
        )
        .await
        .expect("callee query should succeed");
    let hit = response
        .results
        .iter()
        .find(|hit| hit.excerpt.contains("missing_dependency"))
        .expect("unresolved callee edge should be returned");

    assert_eq!(hit.symbol_snapshot_id, None);
}

#[tokio::test]
async fn worktree_overlay_requires_explicit_worktree_ref_for_queries() {
    let repo = FixtureRepo::create("code-overlay");
    repo.write("src/lib.rs", "pub fn retry_policy() -> u32 { 3 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-overlay"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-overlay"),
        )
        .await
        .expect("clean repository should index");

    repo.write(
        "src/lib.rs",
        "pub fn retry_policy() -> u32 { 5 }\npub fn retry_policy_v2() -> u32 { retry_policy() }\n",
    );
    let overlay = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("overlay"),
        )
        .await
        .expect("worktree overlay should index");

    assert!(overlay.summary.resolved_commit_sha.starts_with("worktree:"));
    let clean_query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy_v2",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-clean-overlay"),
        )
        .await
        .expect("clean commit query should stay on the clean snapshot");
    assert!(clean_query.results.is_empty());

    let overlay_query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy_v2",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-overlay"),
        )
        .await
        .expect("explicit worktree query should succeed");
    assert!(
        overlay_query
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
}

#[tokio::test]
async fn git_snapshot_queries_remain_isolated_after_indexing_another_branch() {
    let repo = FixtureRepo::create("code-branch-scope");
    repo.write("src/lib.rs", "pub fn branch_a_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "branch a"]);
    repo.git(["branch", "branch-a"]);
    repo.git(["checkout", "-b", "branch-b"]);
    repo.write("src/lib.rs", "pub fn branch_b_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "branch b"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-branch-scope").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "branch-a"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-branch-a"),
        )
        .await
        .expect("branch A should index");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "branch-b"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-branch-b"),
        )
        .await
        .expect("branch B should index");

    let branch_a = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "branch_a_policy",
                selector("fixture", "branch-a"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-branch-a"),
        )
        .await
        .expect("branch A query should use branch A scope");
    let branch_b = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "branch_b_policy",
                selector("fixture", "branch-b"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-branch-b"),
        )
        .await
        .expect("branch B query should use branch B scope");

    assert!(
        branch_a
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("branch_a_policy"))
    );
    assert!(
        !branch_a
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("branch_b_policy"))
    );
    assert!(
        branch_b
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("branch_b_policy"))
    );
    assert_ne!(branch_a.scope.scope_id, branch_b.scope.scope_id);
}

#[tokio::test]
async fn same_tree_hash_branches_reuse_scope_but_preserve_requested_ref_audit() {
    let repo = FixtureRepo::create("code-same-tree");
    repo.write("src/lib.rs", "pub fn shared_policy() -> u32 { 7 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "shared"]);
    repo.git(["branch", "branch-a"]);
    repo.git(["branch", "branch-b"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-same-tree").await;

    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "branch-a"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-same-tree-a"),
        )
        .await
        .expect("branch A should index");
    let queried = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "shared_policy",
                selector("fixture", "branch-b"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-same-tree-b"),
        )
        .await
        .expect("branch B should reuse same commit/tree scope");

    assert_eq!(queried.scope.requested_ref, "branch-b");
    assert_eq!(queried.scope.scope_id, indexed.scope.scope_id);
    assert_eq!(queried.scope.tree_hash, indexed.scope.tree_hash);
}

async fn query(
    service: &RelayKnowledgeService,
    query: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector("fixture", "HEAD"),
                kind,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query"),
        )
        .await
        .expect("query should succeed")
}

async fn register_fixture_repo(service: &RelayKnowledgeService, repo: &FixtureRepo, name: &str) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context(name),
        )
        .await
        .expect("repository should register");
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
        fs::create_dir_all(path.join("src")).expect("repo directory should be created");
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
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
