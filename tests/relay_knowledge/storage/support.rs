use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    domain::{
        CodeIndexBatch, CodeIndexMode, CodeIndexResourceBudget, CodeIndexSession,
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, SoftwareGlobalKind, SoftwareGlobalRequest,
    },
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    storage::CodeIndexTaskSeed,
};
use rusqlite::Connection;

pub(super) fn runtime_paths() -> RuntimePaths {
    runtime_paths_for_root(&unique_temp_dir("partitioned-sqlite"))
}

pub(super) fn runtime_paths_for_root(root: &Path) -> RuntimePaths {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("temp path should be UTF-8"),
        )],
    )
    .expect("environment should parse");

    RuntimePaths::resolve(&environment.platform, &environment.paths).expect("paths resolve")
}

pub(super) fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

pub(super) fn registration(repository_id: &str, alias: &str) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new(
        repository_id,
        alias,
        format!("/tmp/{alias}"),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration validates")
}

pub(super) fn snapshot(
    repository_id: &str,
    source_scope: &str,
    content: &str,
) -> CodeIndexSnapshot {
    let file = RepositoryCodeFileRecord {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: format!("{repository_id}:src/lib.rs"),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("sha256:{repository_id}"),
        byte_len: content.len(),
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    };
    let chunk = RepositoryCodeChunkRecord {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: format!("{repository_id}:chunk"),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        language_id: file.language_id.clone(),
        content: content.to_owned(),
        byte_range: range(0, content.len()),
        line_range: range(1, 1),
        symbol_snapshot_id: None,
    };

    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: format!("{repository_id}-commit"),
        tree_hash: format!("{repository_id}-tree"),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![chunk],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_commit(
    repository_id: &str,
    source_scope: &str,
    commit: &str,
    content: &str,
) -> CodeIndexSnapshot {
    let mut snapshot = snapshot(repository_id, source_scope, content);
    snapshot.resolved_commit_sha = commit.to_owned();
    snapshot.tree_hash = format!("{commit}-tree");
    snapshot
}

pub(super) fn incremental_snapshot(
    repository_id: &str,
    source_scope: &str,
    base_commit: &str,
    content: &str,
) -> CodeIndexSnapshot {
    let mut snapshot = snapshot(repository_id, source_scope, content);
    snapshot.base_resolved_commit_sha = Some(base_commit.to_owned());
    snapshot.resolved_commit_sha = format!("{repository_id}-next-commit");
    snapshot.tree_hash = format!("{repository_id}-next-tree");
    snapshot.full_replace = false;
    snapshot
}

pub(super) fn session_for_snapshot(snapshot: &CodeIndexSnapshot) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        base_resolved_commit_sha: snapshot.base_resolved_commit_sha.clone(),
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        path_filters: snapshot.path_filters.clone(),
        language_filters: snapshot.language_filters.clone(),
        full_replace: snapshot.full_replace,
        total_path_count: snapshot.files.len(),
        changed_path_count: snapshot.changed_path_count,
        skipped_unchanged_count: snapshot.skipped_unchanged_count,
        deleted_paths: snapshot.deleted_paths.clone(),
        tombstones: snapshot.tombstones.clone(),
        workspaces: snapshot.workspaces.clone(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

pub(super) fn batch_from_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        batch_index: 0,
        parsed_byte_count: snapshot.files.iter().map(|file| file.byte_len).sum(),
        files: snapshot.files,
        symbols: snapshot.symbols,
        references: snapshot.references,
        imports: snapshot.imports,
        dependencies: snapshot.dependencies,
        feature_flags: snapshot.feature_flags,
        chunks: snapshot.chunks,
        diagnostics: snapshot.diagnostics,
    }
}

pub(super) fn code_index_task_seed(
    repository_id: &str,
    alias: &str,
    fingerprint: &str,
    source_scope: &str,
    now_ms: u64,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: repository_id.to_owned(),
        alias: alias.to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("{repository_id}-{source_scope}-commit"),
        tree_hash: format!("{repository_id}-{source_scope}-tree"),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}

pub(super) fn retrieval_request(repository: &str, query: &str) -> CodeRetrievalRequest {
    retrieval_request_for_ref(repository, "HEAD", query)
}

pub(super) fn retrieval_request_for_ref(
    repository: &str,
    ref_selector: &str,
    query: &str,
) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new(repository, ref_selector, Vec::new(), Vec::new())
            .expect("selector validates"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request validates")
}

pub(super) fn software_request(repository: &str, ref_selector: &str) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        CodeRepositorySelector::new(repository, ref_selector, Vec::new(), Vec::new())
            .expect("selector validates"),
        SoftwareGlobalKind::Files,
        FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request validates")
}

fn range(start: usize, end: usize) -> RepositoryCodeRange {
    RepositoryCodeRange::new("range", start, end).expect("range validates")
}

pub(super) fn control_code_file_count(path: &Path) -> usize {
    let connection = Connection::open(path).expect("control db opens");
    connection
        .query_row("SELECT COUNT(*) FROM code_repository_files", [], |row| {
            row.get(0)
        })
        .expect("count succeeds")
}

pub(super) fn catalog_shard_locator(path: &Path, repository_id: &str) -> String {
    let connection = Connection::open(path).expect("control db opens");
    connection
        .query_row(
            "SELECT db_path FROM storage_repository_shards WHERE repository_id = ?1",
            [repository_id],
            |row| row.get(0),
        )
        .expect("catalog row exists")
}
