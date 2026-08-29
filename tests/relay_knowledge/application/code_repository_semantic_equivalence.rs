use std::{path::Path, sync::Arc};

use relay_knowledge::{
    api::CodeRepositoryRegisterRequest,
    application::RelayKnowledgeService,
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexTaskState, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalHit, CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{CodeIndexPublicationStore as _, SqliteGraphStore},
};
use rusqlite::{Connection, params};

#[path = "code_repository_semantic_equivalence_support.rs"]
mod support;
use support::{FixtureRepo, context, service_with_store};

const REFERENCE_BASE_TARGET: &str = r#"pub fn stable_target() -> u32 {
    7
}
"#;
const REFERENCE_FINAL_TARGET: &str = r#"fn target_file_prefix() -> u64 {
    11
}

pub fn stable_target() -> u64 {
    target_file_prefix()
}
"#;
const REFERENCE_CALLER: &str = r#"use crate::stable_target;

pub fn unchanged_caller() -> u64 {
    stable_target() as u64
}
"#;
const REFERENCE_BASE_CALLER: &str = r#"use crate::stable_target;

pub fn unchanged_caller() -> u32 {
    stable_target()
}
"#;
const OLD_EFFECTIVE_VERSION: &str = "7.1.37";
const NEW_EFFECTIVE_VERSION: &str = "7.2.41";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceBinding {
    target_symbol_snapshot_id: Option<String>,
    semantics: ReferenceSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceSemantics {
    reference_path: String,
    reference_name: String,
    reference_kind: String,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
    target_hint: Option<String>,
    target_path: Option<String>,
    target_name: Option<String>,
    target_kind: Option<String>,
    target_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DependencySemantics {
    path: String,
    language_id: String,
    package_name: String,
    requirement: Option<String>,
    resolved_version: Option<String>,
    dependency_group: String,
    source_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyMutation {
    operation: String,
    checkpoint_state: String,
    proof_before: i64,
    path: String,
    package_name: String,
    requirement: Option<String>,
    resolved_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckpointProof {
    state: String,
    committed_fact_row_count: usize,
}

#[tokio::test]
async fn leased_incremental_finalization_rebinds_an_unchanged_reference_like_clean_full() {
    let incremental_repo = FixtureRepo::create("incremental-reference-finalization");
    write_reference_fixture(&incremental_repo, REFERENCE_BASE_TARGET);
    incremental_repo.git(["add", "."]);
    incremental_repo.git(["commit", "-m", "reference base"]);
    let base_commit = incremental_repo.git_text(["rev-parse", "HEAD"]);
    let incremental_database = incremental_repo.path.join("incremental.sqlite3");
    let incremental_store = Arc::new(
        SqliteGraphStore::open(&incremental_database).expect("incremental store should open"),
    );
    let incremental_service = service_with_store(Arc::clone(&incremental_store)).await;
    register_repo(
        &incremental_service,
        &incremental_repo,
        vec!["src".to_owned()],
        "register-incremental-reference",
    )
    .await;
    let base = incremental_service
        .index_code_repository(full_request(), context("full-reference-base-through-task"))
        .await
        .expect("leased base full index should complete");
    let base_binding = reference_binding(&incremental_database, &base.summary.source_scope);
    assert_resolved_target(&base_binding, "u32");

    incremental_repo.write("src/target.rs", REFERENCE_FINAL_TARGET);
    incremental_repo.git(["add", "src/target.rs"]);
    incremental_repo.git(["commit", "-m", "move and change target"]);
    let incremental = incremental_service
        .index_code_repository(
            incremental_request(&base_commit),
            context("leased-incremental-reference-finalization"),
        )
        .await
        .expect("leased incremental finalization should complete");
    let incremental_checkpoint = incremental_store
        .code_index_checkpoint(incremental.summary.source_scope.clone())
        .await
        .expect("incremental reference checkpoint should load")
        .expect("incremental reference checkpoint should exist");
    let receipt = incremental_checkpoint
        .incremental_summary
        .as_ref()
        .expect("reference delta must cross the durable clone-to-finalizer handoff");

    assert_eq!(incremental.summary.changed_path_count, 1);
    assert_eq!(incremental.summary.skipped_unchanged_count, 0);
    assert_eq!(incremental.summary.deleted_path_count, 0);
    assert_eq!(incremental.summary.progress.blob_read_count, 1);
    assert_eq!(incremental.summary.progress.parsed_file_count, 1);
    assert_eq!(incremental_checkpoint.state, "completed");
    assert_eq!(receipt.base_resolved_commit_sha, base_commit);
    assert_eq!(receipt.changed_path_count, 1);
    assert_eq!(receipt.skipped_unchanged_count, 0);
    assert_eq!(receipt.deleted_path_count, 0);
    assert_eq!(receipt.blob_read_count, 1);
    assert_eq!(receipt.parsed_file_count, 1);
    let incremental_binding =
        reference_binding(&incremental_database, &incremental.summary.source_scope);
    assert_resolved_target(&incremental_binding, "u64");
    assert_ne!(
        incremental_binding.target_symbol_snapshot_id, base_binding.target_symbol_snapshot_id,
        "the unchanged caller must not retain the base scope's changed target identity"
    );

    let full_repo = FixtureRepo::create("clean-full-reference-finalization");
    write_reference_fixture(&full_repo, REFERENCE_FINAL_TARGET);
    full_repo.git(["add", "."]);
    full_repo.git(["commit", "-m", "clean full reference target"]);
    let full_database = full_repo.path.join("full.sqlite3");
    let full_store =
        Arc::new(SqliteGraphStore::open(&full_database).expect("clean full store should open"));
    let full_service = service_with_store(full_store).await;
    register_repo(
        &full_service,
        &full_repo,
        vec!["src".to_owned()],
        "register-clean-full-reference",
    )
    .await;
    let full = full_service
        .index_code_repository(full_request(), context("clean-full-reference-finalization"))
        .await
        .expect("clean full finalization should complete");
    let full_binding = reference_binding(&full_database, &full.summary.source_scope);

    assert_eq!(incremental_binding.semantics, full_binding.semantics);
    let hits = reference_hits(&incremental_service, "stable_target").await;
    assert!(hits.iter().any(|hit| {
        hit.path == "src/caller.rs"
            && hit.edge_resolution_state.as_deref() == Some("resolved")
            && hit.edge_target_hint.as_deref() == Some("stable_target")
    }));
}

#[tokio::test]
async fn second_task_adopts_the_same_content_scope_without_reusing_the_first_receipt() {
    let repo = FixtureRepo::create("incremental-scope-adoption");
    repo.write("src/target.rs", REFERENCE_BASE_TARGET);
    repo.write("src/caller.rs", REFERENCE_BASE_CALLER);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "adoption base"]);
    let base_commit = repo.git_text(["rev-parse", "HEAD"]);
    let database = repo.path.join("adoption.sqlite3");
    let store = Arc::new(SqliteGraphStore::open(&database).expect("adoption store should open"));
    let service = service_with_store(Arc::clone(&store)).await;
    register_repo(
        &service,
        &repo,
        vec!["src".to_owned()],
        "register-scope-adoption",
    )
    .await;
    service
        .index_code_repository(full_request(), context("full-adoption-base"))
        .await
        .expect("adoption base should index");

    repo.write("src/caller.rs", REFERENCE_CALLER);
    repo.git(["add", "src/caller.rs"]);
    repo.git(["commit", "-m", "intermediate caller shape"]);
    let intermediate_commit = repo.git_text(["rev-parse", "HEAD"]);
    service
        .index_code_repository(
            incremental_request(&base_commit),
            context("index-intermediate-adoption-base"),
        )
        .await
        .expect("intermediate scope should index");

    repo.write("src/target.rs", REFERENCE_FINAL_TARGET);
    repo.git(["add", "src/target.rs"]);
    repo.git(["commit", "-m", "shared final content"]);
    let first = service
        .index_code_repository(
            incremental_request(&base_commit),
            context("publish-first-task-for-shared-scope"),
        )
        .await
        .expect("first task should publish the shared scope");
    assert_eq!(first.summary.changed_path_count, 2);
    assert_eq!(first.summary.progress.parsed_file_count, 2);
    let first_receipt_task = store
        .code_index_checkpoint(first.summary.source_scope.clone())
        .await
        .expect("first shared-scope checkpoint should load")
        .and_then(|checkpoint| checkpoint.incremental_summary)
        .expect("first shared-scope task should leave a durable receipt")
        .task_id;

    let started = service
        .start_code_repository_index(
            incremental_request(&intermediate_commit),
            context("queue-second-task-for-shared-scope"),
        )
        .await
        .expect("second task should queue for scope adoption");
    let second_task = started.task.expect("second adoption should return a task");
    assert_eq!(second_task.source_scope, first.summary.source_scope);
    assert_ne!(second_task.task_id, first_receipt_task);
    let second = service
        .index_code_repository(
            incremental_request(&intermediate_commit),
            context("run-second-task-for-shared-scope"),
        )
        .await
        .expect("second task should adopt the already-published scope");

    assert_eq!(second.summary.source_scope, first.summary.source_scope);
    assert_eq!(second.summary.base_resolved_commit_sha, None);
    assert_eq!(second.summary.changed_path_count, 0);
    assert_eq!(second.summary.skipped_unchanged_count, 2);
    assert_eq!(second.summary.progress.blob_read_count, 0);
    assert_eq!(second.summary.progress.parsed_file_count, 0);
    assert_eq!(second.summary.progress.sqlite_write_count, 0);
    assert_eq!(second.summary.progress.batch_count, 0);
    let adopted_checkpoint = store
        .code_index_checkpoint(second.summary.source_scope.clone())
        .await
        .expect("adopted checkpoint should load")
        .expect("adopted checkpoint should exist");
    assert_eq!(adopted_checkpoint.state, "completed");
    assert!(adopted_checkpoint.incremental_summary.is_none());
}

#[tokio::test]
async fn leased_incremental_maven_refresh_matches_full_and_replay_preserves_f_minus_d_plus_i() {
    let incremental_repo = FixtureRepo::create("incremental-maven-finalization");
    write_maven_fixture(&incremental_repo, OLD_EFFECTIVE_VERSION);
    incremental_repo.git(["add", "."]);
    incremental_repo.git(["commit", "-m", "maven base"]);
    let base_commit = incremental_repo.git_text(["rev-parse", "HEAD"]);
    let incremental_database = incremental_repo.path.join("incremental.sqlite3");
    let incremental_store = Arc::new(
        SqliteGraphStore::open(&incremental_database).expect("incremental store should open"),
    );
    let incremental_service = service_with_store(Arc::clone(&incremental_store)).await;
    register_repo(
        &incremental_service,
        &incremental_repo,
        Vec::new(),
        "register-incremental-maven",
    )
    .await;
    let base = incremental_service
        .index_code_repository(full_request(), context("full-maven-base-through-task"))
        .await
        .expect("leased Maven base full index should complete");
    let base_checkpoint = incremental_store
        .code_index_checkpoint(base.summary.source_scope.clone())
        .await
        .expect("base Maven checkpoint should load")
        .expect("base Maven checkpoint should exist");
    assert_eq!(base_checkpoint.state, "completed");
    assert!(base_checkpoint.committed_fact_row_count > 0);
    let base_dependencies = dependency_semantics(&incremental_database, &base.summary.source_scope);
    assert!(base_dependencies.iter().any(|dependency| {
        dependency.path == "child/pom.xml"
            && dependency.requirement.as_deref() == Some(OLD_EFFECTIVE_VERSION)
    }));

    incremental_repo.write("pom.xml", &parent_pom(NEW_EFFECTIVE_VERSION));
    incremental_repo.git(["add", "pom.xml"]);
    incremental_repo.git(["commit", "-m", "change inherited dependency version"]);
    let started = incremental_service
        .start_code_repository_index(
            incremental_request(&base_commit),
            context("start-leased-incremental-maven"),
        )
        .await
        .expect("incremental Maven task should queue");
    let queued = started
        .task
        .expect("incremental update should return a task");
    assert!(matches!(&queued.mode, CodeIndexMode::Incremental { .. }));
    install_dependency_refresh_audit_and_completion_fault(&incremental_database, &queued.task_id);

    let completion_error = incremental_service
        .run_code_index_task_once(
            Some(queued.task_id.clone()),
            context("run-leased-incremental-maven"),
        )
        .await
        .expect_err("the test fault should lose the completion response after publication");
    assert!(
        completion_error
            .message
            .contains("simulated response loss after publication")
    );
    let checkpoint = incremental_store
        .code_index_checkpoint(queued.source_scope.clone())
        .await
        .expect("incremental checkpoint should load")
        .expect("incremental checkpoint should exist");
    let receipt = checkpoint
        .incremental_summary
        .as_ref()
        .expect("delta-to-finalizer handoff must retain its summary receipt");
    assert_eq!(receipt.changed_path_count, 1);
    assert_eq!(receipt.skipped_unchanged_count, 0);
    assert_eq!(receipt.deleted_path_count, 0);
    assert_eq!(receipt.blob_read_count, 1);
    assert_eq!(receipt.parsed_file_count, 1);

    let audit = dependency_refresh_audit(&incremental_database, &queued.source_scope);
    let deleted = audit
        .iter()
        .filter(|mutation| mutation.operation == "D")
        .collect::<Vec<_>>();
    assert!(
        !deleted.is_empty(),
        "effective refresh must delete cloned facts"
    );
    let proof_before = deleted[0].proof_before;
    let refresh_state = deleted[0].checkpoint_state.as_str();
    assert!(
        proof_before > 0,
        "refresh must start from a positive durable fact proof"
    );
    assert!(deleted.iter().all(|mutation| {
        mutation.proof_before == proof_before && mutation.checkpoint_state == refresh_state
    }));
    let inserted = audit
        .iter()
        .filter(|mutation| {
            mutation.operation == "I"
                && mutation.proof_before == proof_before
                && mutation.checkpoint_state == refresh_state
        })
        .collect::<Vec<_>>();
    assert!(
        !inserted.is_empty(),
        "effective refresh must insert rebuilt facts"
    );
    assert!(deleted.iter().any(|mutation| {
        mutation.path == "child/pom.xml"
            && mutation.package_name == "org.example:shared-api"
            && mutation.requirement.as_deref() == Some(OLD_EFFECTIVE_VERSION)
    }));
    assert!(inserted.iter().any(|mutation| {
        mutation.path == "child/pom.xml"
            && mutation.package_name == "org.example:shared-api"
            && mutation.requirement.as_deref() == Some(NEW_EFFECTIVE_VERSION)
    }));
    let deleted_fact_count = deleted.len();
    let inserted_fact_count = inserted.len();
    assert!(deleted_fact_count > 0 && inserted_fact_count > 0);
    let proof_before = usize::try_from(proof_before).expect("positive proof should fit usize");
    assert_eq!(
        proof_before,
        base_checkpoint
            .committed_fact_row_count
            .checked_add(receipt.sqlite_write_count)
            .expect("cross-generation handoff proof should fit usize"),
        "the refresh step must inherit the base proof plus its delta fact upper bound"
    );
    let expected_proof = proof_before
        .checked_sub(deleted_fact_count)
        .and_then(|proof| proof.checked_add(inserted_fact_count))
        .expect("F-D+I should remain in range");
    assert_eq!(checkpoint.state, "completed");
    assert_eq!(checkpoint.committed_fact_row_count, expected_proof);
    assert!(
        checkpoint.committed_fact_row_count
            >= count_scope_fact_rows(&incremental_database, &queued.source_scope),
        "the durable proof is an upper bound, not necessarily the exact target fact count"
    );

    let incremental_dependencies =
        dependency_semantics(&incremental_database, &queued.source_scope);
    assert!(incremental_dependencies.iter().any(|dependency| {
        dependency.path == "child/pom.xml"
            && dependency.requirement.as_deref() == Some(NEW_EFFECTIVE_VERSION)
    }));
    assert!(!incremental_dependencies.iter().any(|dependency| {
        dependency.package_name == "org.example:shared-api"
            && dependency.requirement.as_deref() == Some(OLD_EFFECTIVE_VERSION)
    }));
    assert_eq!(
        exact_dependency_search_owner_count(
            &incremental_database,
            &queued.source_scope,
            OLD_EFFECTIVE_VERSION,
        ),
        0
    );
    assert!(
        exact_dependency_search_owner_count(
            &incremental_database,
            &queued.source_scope,
            NEW_EFFECTIVE_VERSION,
        ) > 0
    );
    let proof_before_replay = checkpoint_proof(&incremental_database, &queued.source_scope);
    let audit_count_before_replay = audit.len();
    let search_count_before_replay = exact_dependency_search_owner_count(
        &incremental_database,
        &queued.source_scope,
        NEW_EFFECTIVE_VERSION,
    );
    prepare_response_lost_replay(&incremental_database, &queued.task_id);
    mark_software_projection_stale(&incremental_database, &queued.source_scope);
    let replay = incremental_service
        .run_code_index_task_once(
            Some(queued.task_id.clone()),
            context("replay-after-lost-maven-response"),
        )
        .await
        .expect("published task replay should reconcile")
        .expect("expired response-lost attempt should be reclaimed");
    assert_eq!(replay.state, CodeIndexTaskState::Succeeded);
    assert!(replay.attempt_count > queued.attempt_count);
    assert_eq!(
        checkpoint_proof(&incremental_database, &queued.source_scope),
        proof_before_replay
    );
    assert_eq!(
        dependency_refresh_audit(&incremental_database, &queued.source_scope).len(),
        audit_count_before_replay
    );
    assert_eq!(
        exact_dependency_search_owner_count(
            &incremental_database,
            &queued.source_scope,
            NEW_EFFECTIVE_VERSION,
        ),
        search_count_before_replay
    );
    assert!(!software_projection_is_stale(
        &incremental_database,
        &queued.source_scope
    ));

    let sbom = sbom_hits(&incremental_service, "org.example:shared-api").await;
    assert!(
        sbom.iter().any(|hit| {
            hit.path == "child/pom.xml" && hit.excerpt.contains(NEW_EFFECTIVE_VERSION)
        })
    );
    assert!(
        !sbom
            .iter()
            .any(|hit| hit.excerpt.contains(OLD_EFFECTIVE_VERSION))
    );

    let full_repo = FixtureRepo::create("clean-full-maven-finalization");
    write_maven_fixture(&full_repo, NEW_EFFECTIVE_VERSION);
    full_repo.git(["add", "."]);
    full_repo.git(["commit", "-m", "clean full Maven model"]);
    let full_database = full_repo.path.join("full.sqlite3");
    let full_store =
        Arc::new(SqliteGraphStore::open(&full_database).expect("clean full store should open"));
    let full_service = service_with_store(full_store).await;
    register_repo(
        &full_service,
        &full_repo,
        Vec::new(),
        "register-clean-full-maven",
    )
    .await;
    let full = full_service
        .index_code_repository(full_request(), context("clean-full-maven-finalization"))
        .await
        .expect("clean full Maven finalization should complete");
    let full_dependencies = dependency_semantics(&full_database, &full.summary.source_scope);
    assert_eq!(incremental_dependencies, full_dependencies);
}

fn write_reference_fixture(repo: &FixtureRepo, target: &str) {
    repo.write("src/target.rs", target);
    repo.write("src/caller.rs", REFERENCE_CALLER);
}

fn write_maven_fixture(repo: &FixtureRepo, version: &str) {
    repo.write("pom.xml", &parent_pom(version));
    repo.write("child/pom.xml", child_pom());
}

fn parent_pom(version: &str) -> String {
    format!(
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <properties><shared.version>{version}</shared.version></properties>
  <modules><module>child</module></modules>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>shared-api</artifactId>
        <version>${{shared.version}}</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#
    )
}

fn child_pom() -> &'static str {
    r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>child</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>shared-api</artifactId>
    </dependency>
  </dependencies>
</project>
"#
}

async fn register_repo(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    path_filters: Vec<String>,
    request_name: &str,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters,
                language_filters: Vec::new(),
            },
            context(request_name),
        )
        .await
        .expect("fixture repository should register");
}

fn full_request() -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector(),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    }
}

fn incremental_request(base_commit: &str) -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector(),
        mode: CodeIndexMode::incremental(base_commit, "HEAD")
            .expect("incremental mode should validate"),
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    }
}

fn selector() -> CodeRepositorySelector {
    CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn reference_binding(database: &Path, source_scope: &str) -> ReferenceBinding {
    let connection = Connection::open(database).expect("reference observer should open");
    connection
        .query_row(
            "SELECT reference.target_symbol_snapshot_id, reference.path, reference.name,
                    reference.kind, reference.resolution_state,
                    reference.confidence_basis_points, reference.confidence_tier,
                    reference.target_hint, target.path, target.name, target.kind, target.signature
             FROM code_repository_references reference
             LEFT JOIN code_repository_symbols target
               ON target.source_scope = reference.source_scope
              AND target.symbol_snapshot_id = reference.target_symbol_snapshot_id
             WHERE reference.source_scope = ?1
               AND reference.path = 'src/caller.rs'
               AND reference.name = 'stable_target'
               AND reference.kind = 'call'
             ORDER BY reference.line_start ASC
             LIMIT 1",
            [source_scope],
            |row| {
                Ok(ReferenceBinding {
                    target_symbol_snapshot_id: row.get(0)?,
                    semantics: ReferenceSemantics {
                        reference_path: row.get(1)?,
                        reference_name: row.get(2)?,
                        reference_kind: row.get(3)?,
                        resolution_state: row.get(4)?,
                        confidence_basis_points: row.get(5)?,
                        confidence_tier: row.get(6)?,
                        target_hint: row.get(7)?,
                        target_path: row.get(8)?,
                        target_name: row.get(9)?,
                        target_kind: row.get(10)?,
                        target_signature: row.get(11)?,
                    },
                })
            },
        )
        .expect("stable_target call reference should exist")
}

fn assert_resolved_target(binding: &ReferenceBinding, signature_fragment: &str) {
    assert_eq!(binding.semantics.resolution_state, "resolved");
    assert_eq!(
        binding.semantics.target_hint.as_deref(),
        Some("stable_target")
    );
    assert_eq!(
        binding.semantics.target_path.as_deref(),
        Some("src/target.rs")
    );
    assert_eq!(
        binding.semantics.target_name.as_deref(),
        Some("stable_target")
    );
    assert!(
        binding
            .semantics
            .target_signature
            .as_deref()
            .is_some_and(|signature| signature.contains(signature_fragment))
    );
}

async fn reference_hits(service: &RelayKnowledgeService, query: &str) -> Vec<CodeRetrievalHit> {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector(),
                CodeQueryKind::References,
                20,
                FreshnessPolicy::WaitUntilFresh,
            )
            .expect("reference request should validate"),
            context("query-incremental-reference-binding"),
        )
        .await
        .expect("reference search should succeed")
        .results
}

async fn sbom_hits(service: &RelayKnowledgeService, query: &str) -> Vec<CodeRetrievalHit> {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector(),
                CodeQueryKind::Sbom,
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .expect("SBOM request should validate"),
            context("query-incremental-maven-dependency"),
        )
        .await
        .expect("SBOM search should succeed")
        .results
}

fn dependency_semantics(database: &Path, source_scope: &str) -> Vec<DependencySemantics> {
    let connection = Connection::open(database).expect("dependency observer should open");
    let mut statement = connection
        .prepare(
            "SELECT path, language_id, package_name, requirement, resolved_version,
                    dependency_group, source_kind
             FROM code_repository_dependencies
             WHERE source_scope = ?1 AND ecosystem = 'maven'
               AND package_name = 'org.example:shared-api'
             ORDER BY path, language_id, dependency_group, requirement",
        )
        .expect("dependency query should prepare");
    statement
        .query_map([source_scope], |row| {
            Ok(DependencySemantics {
                path: row.get(0)?,
                language_id: row.get(1)?,
                package_name: row.get(2)?,
                requirement: row.get(3)?,
                resolved_version: row.get(4)?,
                dependency_group: row.get(5)?,
                source_kind: row.get(6)?,
            })
        })
        .expect("dependency query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("dependency rows should decode")
}

fn install_dependency_refresh_audit_and_completion_fault(database: &Path, task_id: &str) {
    let connection = Connection::open(database).expect("audit installer should open");
    connection
        .execute_batch(
            "CREATE TABLE test_maven_dependency_refresh_audit (
                 operation TEXT NOT NULL,
                 source_scope TEXT NOT NULL,
                 checkpoint_state TEXT NOT NULL,
                 proof_before INTEGER NOT NULL,
                 path TEXT NOT NULL,
                 package_name TEXT NOT NULL,
                 requirement TEXT,
                 resolved_version TEXT
             );
             CREATE TABLE test_task_completion_fault (
                 task_id TEXT PRIMARY KEY,
                 armed INTEGER NOT NULL
             );
             CREATE TRIGGER test_maven_dependency_refresh_delete
             AFTER DELETE ON code_repository_dependencies
             WHEN OLD.ecosystem = 'maven' AND OLD.source_kind = 'pom.xml'
             BEGIN
                 INSERT INTO test_maven_dependency_refresh_audit
                 VALUES (
                     'D', OLD.source_scope,
                     coalesce((SELECT state FROM code_repository_index_checkpoints
                               WHERE source_scope = OLD.source_scope), 'missing'),
                     coalesce((SELECT committed_fact_row_count
                               FROM code_repository_index_checkpoints
                               WHERE source_scope = OLD.source_scope), -1),
                     OLD.path, OLD.package_name, OLD.requirement, OLD.resolved_version
                 );
             END;
             CREATE TRIGGER test_maven_dependency_refresh_insert
             AFTER INSERT ON code_repository_dependencies
             WHEN NEW.ecosystem = 'maven' AND NEW.source_kind = 'pom.xml'
             BEGIN
                 INSERT INTO test_maven_dependency_refresh_audit
                 VALUES (
                     'I', NEW.source_scope,
                     coalesce((SELECT state FROM code_repository_index_checkpoints
                               WHERE source_scope = NEW.source_scope), 'missing'),
                     coalesce((SELECT committed_fact_row_count
                               FROM code_repository_index_checkpoints
                               WHERE source_scope = NEW.source_scope), -1),
                     NEW.path, NEW.package_name, NEW.requirement, NEW.resolved_version
                 );
             END;
             CREATE TRIGGER test_lose_task_completion_response
             BEFORE UPDATE OF state ON code_repository_index_tasks
             WHEN NEW.state = 'succeeded'
               AND EXISTS (
                   SELECT 1 FROM test_task_completion_fault fault
                   WHERE fault.task_id = NEW.task_id AND fault.armed = 1
               )
             BEGIN
                 SELECT RAISE(ABORT, 'simulated response loss after publication');
             END;",
        )
        .expect("dependency refresh audit should install");
    connection
        .execute(
            "INSERT INTO test_task_completion_fault (task_id, armed) VALUES (?1, 1)",
            [task_id],
        )
        .expect("completion fault should arm");
}

fn prepare_response_lost_replay(database: &Path, task_id: &str) {
    let connection = Connection::open(database).expect("replay preparer should open");
    let disarmed = connection
        .execute(
            "UPDATE test_task_completion_fault SET armed = 0 WHERE task_id = ?1",
            [task_id],
        )
        .expect("completion fault should disarm");
    assert_eq!(disarmed, 1);
    let expired = connection
        .execute(
            "UPDATE code_repository_index_tasks
             SET lease_expires_at_ms = 0
             WHERE task_id = ?1 AND state = 'running'",
            [task_id],
        )
        .expect("response-lost lease should expire");
    assert_eq!(expired, 1);
}

fn mark_software_projection_stale(database: &Path, source_scope: &str) {
    let connection = Connection::open(database).expect("software stale marker should open");
    let changed = connection
        .execute(
            "UPDATE software_global_status SET stale = 1 WHERE source_scope = ?1",
            [source_scope],
        )
        .expect("software projection should become stale");
    assert_eq!(changed, 1);
}

fn software_projection_is_stale(database: &Path, source_scope: &str) -> bool {
    Connection::open(database)
        .expect("software stale observer should open")
        .query_row(
            "SELECT stale FROM software_global_status WHERE source_scope = ?1",
            [source_scope],
            |row| row.get(0),
        )
        .expect("software projection status should exist")
}

fn dependency_refresh_audit(database: &Path, source_scope: &str) -> Vec<DependencyMutation> {
    let connection = Connection::open(database).expect("audit observer should open");
    let mut statement = connection
        .prepare(
            "SELECT operation, checkpoint_state, proof_before, path, package_name,
                    requirement, resolved_version
             FROM test_maven_dependency_refresh_audit
             WHERE source_scope = ?1
             ORDER BY rowid",
        )
        .expect("audit query should prepare");
    statement
        .query_map([source_scope], |row| {
            Ok(DependencyMutation {
                operation: row.get(0)?,
                checkpoint_state: row.get(1)?,
                proof_before: row.get(2)?,
                path: row.get(3)?,
                package_name: row.get(4)?,
                requirement: row.get(5)?,
                resolved_version: row.get(6)?,
            })
        })
        .expect("audit query should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("audit rows should decode")
}

fn checkpoint_proof(database: &Path, source_scope: &str) -> CheckpointProof {
    let connection = Connection::open(database).expect("checkpoint observer should open");
    connection
        .query_row(
            "SELECT state, committed_fact_row_count
             FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            [source_scope],
            |row| {
                Ok(CheckpointProof {
                    state: row.get(0)?,
                    committed_fact_row_count: row.get(1)?,
                })
            },
        )
        .expect("checkpoint proof should exist")
}

fn count_scope_fact_rows(database: &Path, source_scope: &str) -> usize {
    const FACT_TABLES: [&str; 10] = [
        "code_repository_files",
        "code_repository_symbols",
        "code_repository_references",
        "code_repository_imports",
        "code_repository_dependencies",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_routes",
        "code_repository_chunks",
        "code_repository_file_diagnostics",
    ];
    let connection = Connection::open(database).expect("fact proof observer should open");
    FACT_TABLES
        .iter()
        .map(|table| {
            connection
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE source_scope = ?1"),
                    [source_scope],
                    |row| row.get::<_, usize>(0),
                )
                .expect("scope fact row count should load")
        })
        .sum()
}

fn exact_dependency_search_owner_count(
    database: &Path,
    source_scope: &str,
    version: &str,
) -> usize {
    let connection = Connection::open(database).expect("search observer should open");
    connection
        .query_row(
            "SELECT count(*)
             FROM code_repository_search search
             JOIN code_repository_search_metadata metadata
               ON metadata.search_rowid = search.rowid
              AND metadata.source_scope = search.source_scope
              AND metadata.document_kind = search.document_kind
              AND metadata.record_id = search.record_id
              AND metadata.path = search.path
             WHERE search.source_scope = ?1
               AND search.document_kind = 'dependency'
               AND search.content LIKE '%' || ?2 || '%'",
            params![source_scope, version],
            |row| row.get(0),
        )
        .expect("exact dependency search owner count should load")
}
