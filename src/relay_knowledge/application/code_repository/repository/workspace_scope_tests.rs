//! Config-aware source-scope fast-path and preview regressions.

use std::sync::Arc;

use crate::{
    api::CodeRepositoryRegisterRequest,
    code::{reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root},
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
        FreshnessPolicy, code_snapshot_scope_id_with_workspace_detection,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

use super::test_support::*;

#[tokio::test]
async fn equivalent_enabled_workspace_config_reuses_the_exact_published_scope() {
    let repo = FixtureRepo::create("workspace-scope-fast-path");
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"member-a\", \"member-b\"]\nresolver = \"2\"\n",
    );
    repo.write(
        "member-a/Cargo.toml",
        "[package]\nname = \"member-a\"\nversion = \"0.1.0\"\n",
    );
    repo.write("member-a/src/lib.rs", "pub fn member_a() {}\n");
    repo.write(
        "member-b/Cargo.toml",
        "[package]\nname = \"member-b\"\nversion = \"0.1.0\"\n",
    );
    repo.write("member-b/src/lib.rs", "pub fn member_b() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "workspace"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(Arc::clone(&store)).await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-workspace-scope-fast-path"),
        )
        .await
        .expect("workspace fixture should register without hiding root manifests");
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        workspace_detection: CodeWorkspaceDetectionConfig::enabled_all(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    };
    let first = service
        .index_code_repository(request.clone(), context("index-workspace-first"))
        .await
        .expect("workspace-aware index should publish");
    let auto_set_alias = format!("{}-auto-workspace", first.summary.repository_id);
    let before_set = store
        .code_repository_set(auto_set_alias.clone())
        .await
        .expect("workspace set should load")
        .expect("fixture should create an auto workspace set");
    let before_edges = store
        .code_repository_set_cross_edges(before_set.set_id.clone())
        .await
        .expect("workspace edges should load");

    let observed_root = repo.path.canonicalize().expect("root should canonicalize");
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let equivalent = CodeIndexRequest {
        workspace_detection: CodeWorkspaceDetectionConfig {
            enabled: true,
            supported_formats: vec![
                CodeMonorepoWorkspaceFormat::CargoWorkspace,
                CodeMonorepoWorkspaceFormat::Pnpm,
                CodeMonorepoWorkspaceFormat::GoModules,
                CodeMonorepoWorkspaceFormat::CargoWorkspace,
            ],
        },
        ..request
    };
    let repeated = service
        .start_code_repository_index(equivalent, context("start-workspace-repeat"))
        .await
        .expect("equivalent workspace config should reuse publication");

    assert!(repeated.task.is_none());
    assert_eq!(
        repeated.scope.scope_id, first.summary.source_scope,
        "canonical workspace masks must address the same scope"
    );
    assert_eq!(
        repeated
            .summary
            .as_ref()
            .expect("fast path should return a summary")
            .progress
            .blob_read_count,
        0
    );
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);
    assert_eq!(
        store
            .code_repository_set(auto_set_alias)
            .await
            .expect("reused workspace set should load"),
        Some(before_set.clone())
    );
    assert_eq!(
        store
            .code_repository_set_cross_edges(before_set.set_id)
            .await
            .expect("reused workspace edges should load"),
        before_edges
    );
}

#[tokio::test]
async fn scope_preview_uses_the_prospective_ref_and_workspace_identity() {
    let repo = FixtureRepo::create("prospective-workspace-scope-preview");
    repo.write("src/lib.rs", "pub fn version_a() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "version-a"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-prospective-preview").await;
    service
        .index_code_repository(request("fixture", "HEAD"), context("index-preview-a"))
        .await
        .expect("version A should publish");
    repo.write("src/lib.rs", "pub fn version_b() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "version-b"]);
    let disabled = request("fixture", "HEAD");
    let enabled = CodeIndexRequest {
        workspace_detection: CodeWorkspaceDetectionConfig::enabled_all(),
        ..disabled.clone()
    };
    let disabled_preview = service
        .preview_code_repository_scope(disabled, context("preview-b-disabled"))
        .await
        .expect("prospective disabled preview should resolve B");
    let enabled_preview = service
        .preview_code_repository_scope(enabled.clone(), context("preview-b-enabled"))
        .await
        .expect("prospective enabled preview should resolve B");

    assert_eq!(
        enabled_preview.scope.resolved_commit_sha,
        enabled_preview.preview.resolved_commit_sha
    );
    assert_eq!(
        enabled_preview.scope.tree_hash,
        enabled_preview.preview.tree_hash
    );
    assert_eq!(
        enabled_preview.scope.indexed_file_count,
        enabled_preview.preview.selected_file_count
    );
    assert_eq!(
        enabled_preview.scope.scope_id,
        code_snapshot_scope_id_with_workspace_detection(
            &enabled_preview.preview.repository_id,
            &enabled_preview.preview.tree_hash,
            &enabled_preview.scope.path_filters,
            &enabled_preview.scope.language_filters,
            &enabled.workspace_detection,
        )
    );
    assert_ne!(
        enabled_preview.scope.scope_id,
        disabled_preview.scope.scope_id
    );
}
