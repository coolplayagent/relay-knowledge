use std::{fs, path::Path};

use serde_json::Value;

use crate::{
    command::{CommandResult, CommandSpec},
    evaluator::runtime::{concurrency::run_limited, contracts::EvalRuntime},
};

const GIT_FILE_COUNT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IndexExpectation {
    pub(super) scope_id: String,
    pub(super) repository_id: String,
    pub(super) alias: String,
    pub(super) requested_ref: String,
    pub(super) resolved_commit_sha: String,
    pub(super) tree_hash: String,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
    pub(super) selected_file_count: u64,
    pub(super) observed_git_file_count: Option<u64>,
    pub(super) declared_expected_file_count: Option<u64>,
}

impl IndexExpectation {
    pub(super) fn from_preview(
        repo_name: &str,
        repo_config: &Value,
        expected_ref: &str,
        payload: &Value,
    ) -> Result<Self, CommandResult> {
        let declared_expected_file_count = repo_config
            .get("expected_file_count")
            .and_then(Value::as_u64);
        let expectation = Self {
            scope_id: string_at(payload, "/scope/scope_id"),
            repository_id: string_at(payload, "/preview/repository_id"),
            alias: string_at(payload, "/preview/alias"),
            requested_ref: string_at(payload, "/preview/requested_ref"),
            resolved_commit_sha: string_at(payload, "/preview/resolved_commit_sha"),
            tree_hash: string_at(payload, "/preview/tree_hash"),
            path_filters: string_vec_at(payload, "/scope/path_filters"),
            language_filters: string_vec_at(payload, "/scope/language_filters"),
            selected_file_count: payload
                .pointer("/preview/selected_file_count")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            observed_git_file_count: repo_config
                .get("observed_git_file_count")
                .and_then(Value::as_u64),
            declared_expected_file_count,
        };
        let fields_present = !expectation.scope_id.is_empty()
            && !expectation.repository_id.is_empty()
            && !expectation.alias.is_empty()
            && !expectation.requested_ref.is_empty()
            && !expectation.resolved_commit_sha.is_empty()
            && !expectation.tree_hash.is_empty();
        let preview_scope_identity_matches = string_at(payload, "/scope/repository_id")
            == expectation.repository_id
            && string_at(payload, "/scope/alias") == expectation.alias
            && string_at(payload, "/scope/requested_ref") == expectation.requested_ref
            && string_at(payload, "/scope/resolved_commit_sha") == expectation.resolved_commit_sha
            && string_at(payload, "/scope/tree_hash") == expectation.tree_hash;
        let declared_matches = declared_expected_file_count
            .is_none_or(|declared| declared == expectation.selected_file_count);
        let passed = fields_present
            && preview_scope_identity_matches
            && expectation.selected_file_count > 0
            && expectation.requested_ref == expected_ref
            && declared_matches;
        if passed {
            return Ok(expectation);
        }
        Err(validation_result(
            format!("{repo_name}_scope_preview_expectation"),
            "scope-preview-expectation",
            false,
            serde_json::json!({
                "expected_ref": expected_ref,
                "fields_present": fields_present,
                "preview_scope_identity_matches": preview_scope_identity_matches,
                "declared_matches_selected": declared_matches,
                "expectation": expectation.evidence(),
            }),
        ))
    }

    pub(super) fn validation_command(&self, repo_name: &str) -> CommandResult {
        validation_result(
            format!("{repo_name}_scope_preview_expectation"),
            "scope-preview-expectation",
            true,
            self.evidence(),
        )
    }

    fn evidence(&self) -> Value {
        serde_json::json!({
            "scope_id": self.scope_id,
            "repository_id": self.repository_id,
            "alias": self.alias,
            "requested_ref": self.requested_ref,
            "resolved_commit_sha": self.resolved_commit_sha,
            "tree_hash": self.tree_hash,
            "path_filters": self.path_filters,
            "language_filters": self.language_filters,
            "selected_file_count": self.selected_file_count,
            "observed_git_file_count": self.observed_git_file_count,
            "declared_expected_file_count": self.declared_expected_file_count,
        })
    }
}

pub(super) fn cold_index_completion_validation(
    repo_name: &str,
    repo_config: &Value,
    expectation: &IndexExpectation,
    payload: &Value,
) -> CommandResult {
    let minimum_files = repo_config
        .get("cold_index_min_file_count")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let identity = IndexResultIdentity::from_payload(payload);
    let indexed_files = u64_at(payload, "/status/indexed_file_count");
    let task_state = str_at(payload, "/task/state").unwrap_or("missing");
    let task_mode = str_at(payload, "/task/mode").unwrap_or("missing");
    let checkpoint_state = str_at(payload, "/checkpoint/state").unwrap_or("missing");
    let checkpoint_total_paths = u64_at(payload, "/checkpoint/total_path_count");
    let checkpoint_committed_files = u64_at(payload, "/checkpoint/committed_file_count");
    let repository_state = str_at(payload, "/status/state").unwrap_or("missing");
    let repository_stale = payload.pointer("/status/stale").and_then(Value::as_bool);
    let identity_matches = identity.matches_without_checkpoint(expectation)
        && identity.checkpoint_matches(expectation);
    let passed = expectation.selected_file_count >= minimum_files
        && indexed_files == expectation.selected_file_count
        && task_state == "succeeded"
        && task_mode == "full"
        && checkpoint_state == "completed"
        && checkpoint_total_paths == expectation.selected_file_count
        && checkpoint_committed_files == checkpoint_total_paths
        && repository_state == "fresh"
        && repository_stale == Some(false)
        && identity_matches;
    let evidence = serde_json::json!({
        "minimum_files": minimum_files,
        "selected_file_count": expectation.selected_file_count,
        "indexed_files": indexed_files,
        "task_state": task_state,
        "task_mode": task_mode,
        "checkpoint_state": checkpoint_state,
        "checkpoint_total_paths": checkpoint_total_paths,
        "checkpoint_committed_files": checkpoint_committed_files,
        "repository_state": repository_state,
        "repository_stale": repository_stale,
        "identity_matches": identity_matches,
        "expected_identity": expectation.evidence(),
        "result_identity": identity.evidence(),
    });
    validation_result(
        format!("{repo_name}_cold_index_completion"),
        "cold-index-completion",
        passed,
        evidence,
    )
}

pub(super) fn incremental_index_completion_validation(
    repo_name: &str,
    repo_config: &Value,
    expected_changed_paths: usize,
    expected_base_ref: &str,
    expectation: &IndexExpectation,
    payload: &Value,
) -> CommandResult {
    let changed_paths = u64_at(payload, "/summary/changed_path_count");
    let blob_reads =
        optional_u64_at(payload, "/summary/progress/blob_read_count").unwrap_or(u64::MAX);
    let parsed_files =
        optional_u64_at(payload, "/summary/progress/parsed_file_count").unwrap_or(u64::MAX);
    let max_blob_reads = repo_config
        .get("incremental_max_blob_reads")
        .and_then(Value::as_u64)
        .unwrap_or(expected_changed_paths as u64);
    let max_parsed_files = repo_config
        .get("incremental_max_parsed_files")
        .and_then(Value::as_u64)
        .unwrap_or(expected_changed_paths as u64);
    let task_state = str_at(payload, "/task/state").unwrap_or("missing");
    let checkpoint_present = payload
        .pointer("/checkpoint")
        .is_some_and(|checkpoint| !checkpoint.is_null());
    let checkpoint_state = str_at(payload, "/checkpoint/state").unwrap_or("missing");
    let checkpoint_total_paths = u64_at(payload, "/checkpoint/total_path_count");
    let checkpoint_committed_files = u64_at(payload, "/checkpoint/committed_file_count");
    let task_id = str_at(payload, "/task/task_id").unwrap_or("missing");
    let receipt_task_id =
        str_at(payload, "/checkpoint/incremental_summary/task_id").unwrap_or("missing");
    let receipt_base_ref = str_at(
        payload,
        "/checkpoint/incremental_summary/base_resolved_commit_sha",
    )
    .unwrap_or("missing");
    let receipt_changed_paths = u64_at(
        payload,
        "/checkpoint/incremental_summary/changed_path_count",
    );
    let receipt_blob_reads = u64_at(payload, "/checkpoint/incremental_summary/blob_read_count");
    let receipt_parsed_files = u64_at(payload, "/checkpoint/incremental_summary/parsed_file_count");
    let indexed_files = u64_at(payload, "/status/indexed_file_count");
    let repository_state = str_at(payload, "/status/state").unwrap_or("missing");
    let repository_stale = payload.pointer("/status/stale").and_then(Value::as_bool);
    let identity = IndexResultIdentity::from_payload(payload);
    let identity_matches = identity.matches_without_checkpoint(expectation);
    let incremental_identity_matches = identity.summary_base_resolved_commit_sha
        == expected_base_ref
        && identity.task_incremental_base_ref == expected_base_ref
        && identity.task_incremental_head_ref == expectation.requested_ref;
    let receipt_matches = !checkpoint_present
        || (receipt_task_id == task_id
            && receipt_base_ref == expected_base_ref
            && receipt_changed_paths == changed_paths
            && receipt_blob_reads == blob_reads
            && receipt_parsed_files == parsed_files);
    let checkpoint_matches = !checkpoint_present
        || (checkpoint_state == "completed"
            && checkpoint_committed_files == checkpoint_total_paths
            && checkpoint_total_paths == expectation.selected_file_count
            && receipt_matches
            && identity.checkpoint_matches(expectation));
    let passed = changed_paths == expected_changed_paths as u64
        && blob_reads <= max_blob_reads
        && parsed_files <= max_parsed_files
        && task_state == "succeeded"
        && indexed_files == expectation.selected_file_count
        && repository_state == "fresh"
        && repository_stale == Some(false)
        && identity_matches
        && incremental_identity_matches
        && checkpoint_matches;
    let evidence = serde_json::json!({
        "expected_changed_paths": expected_changed_paths,
        "changed_paths": changed_paths,
        "blob_reads": blob_reads,
        "max_blob_reads": max_blob_reads,
        "parsed_files": parsed_files,
        "max_parsed_files": max_parsed_files,
        "expected_head_ref": expectation.requested_ref,
        "expected_base_ref": expected_base_ref,
        "expected_resolved_commit_sha": expectation.resolved_commit_sha,
        "selected_file_count": expectation.selected_file_count,
        "indexed_files": indexed_files,
        "task_state": task_state,
        "checkpoint_present": checkpoint_present,
        "checkpoint_state": checkpoint_state,
        "checkpoint_total_paths": checkpoint_total_paths,
        "checkpoint_committed_files": checkpoint_committed_files,
        "receipt_task_id": receipt_task_id,
        "receipt_base_ref": receipt_base_ref,
        "receipt_changed_paths": receipt_changed_paths,
        "receipt_blob_reads": receipt_blob_reads,
        "receipt_parsed_files": receipt_parsed_files,
        "receipt_matches": receipt_matches,
        "repository_state": repository_state,
        "repository_stale": repository_stale,
        "identity_matches": identity_matches,
        "incremental_identity_matches": incremental_identity_matches,
        "checkpoint_matches": checkpoint_matches,
        "expected_identity": expectation.evidence(),
        "result_identity": identity.evidence(),
    });
    validation_result(
        format!("{repo_name}_incremental_index_completion"),
        "incremental-index-completion",
        passed,
        evidence,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexResultIdentity {
    scope_scope_id: String,
    scope_repository_id: String,
    scope_alias: String,
    scope_requested_ref: String,
    scope_resolved_commit_sha: String,
    scope_tree_hash: String,
    scope_indexed_file_count: u64,
    scope_path_filters: Vec<String>,
    scope_language_filters: Vec<String>,
    task_repository_id: String,
    task_alias: String,
    task_ref_selector: String,
    task_resolved_commit_sha: String,
    task_tree_hash: String,
    task_source_scope: String,
    task_path_filters: Vec<String>,
    task_language_filters: Vec<String>,
    task_incremental_base_ref: String,
    task_incremental_head_ref: String,
    summary_repository_id: String,
    summary_source_scope: String,
    summary_base_resolved_commit_sha: String,
    summary_resolved_commit_sha: String,
    summary_tree_hash: String,
    summary_indexed_file_count: u64,
    checkpoint_repository_id: String,
    checkpoint_source_scope: String,
    status_repository_id: String,
    status_alias: String,
    status_last_indexed_scope_id: String,
    status_last_indexed_commit: String,
    status_tree_hash: String,
    status_path_filters: Vec<String>,
    status_language_filters: Vec<String>,
}

impl IndexResultIdentity {
    fn from_payload(payload: &Value) -> Self {
        Self {
            scope_scope_id: string_at(payload, "/scope/scope_id"),
            scope_repository_id: string_at(payload, "/scope/repository_id"),
            scope_alias: string_at(payload, "/scope/alias"),
            scope_requested_ref: string_at(payload, "/scope/requested_ref"),
            scope_resolved_commit_sha: string_at(payload, "/scope/resolved_commit_sha"),
            scope_tree_hash: string_at(payload, "/scope/tree_hash"),
            scope_indexed_file_count: u64_at(payload, "/scope/indexed_file_count"),
            scope_path_filters: string_vec_at(payload, "/scope/path_filters"),
            scope_language_filters: string_vec_at(payload, "/scope/language_filters"),
            task_repository_id: string_at(payload, "/task/repository_id"),
            task_alias: string_at(payload, "/task/alias"),
            task_ref_selector: string_at(payload, "/task/ref_selector"),
            task_resolved_commit_sha: string_at(payload, "/task/resolved_commit_sha"),
            task_tree_hash: string_at(payload, "/task/tree_hash"),
            task_source_scope: string_at(payload, "/task/source_scope"),
            task_path_filters: string_vec_at(payload, "/task/path_filters"),
            task_language_filters: string_vec_at(payload, "/task/language_filters"),
            task_incremental_base_ref: string_at(payload, "/task/mode/incremental/base_ref"),
            task_incremental_head_ref: string_at(payload, "/task/mode/incremental/head_ref"),
            summary_repository_id: string_at(payload, "/summary/repository_id"),
            summary_source_scope: string_at(payload, "/summary/source_scope"),
            summary_base_resolved_commit_sha: string_at(
                payload,
                "/summary/base_resolved_commit_sha",
            ),
            summary_resolved_commit_sha: string_at(payload, "/summary/resolved_commit_sha"),
            summary_tree_hash: string_at(payload, "/summary/tree_hash"),
            summary_indexed_file_count: u64_at(payload, "/summary/indexed_file_count"),
            checkpoint_repository_id: string_at(payload, "/checkpoint/repository_id"),
            checkpoint_source_scope: string_at(payload, "/checkpoint/source_scope"),
            status_repository_id: string_at(payload, "/status/repository_id"),
            status_alias: string_at(payload, "/status/alias"),
            status_last_indexed_scope_id: string_at(payload, "/status/last_indexed_scope_id"),
            status_last_indexed_commit: string_at(payload, "/status/last_indexed_commit"),
            status_tree_hash: string_at(payload, "/status/tree_hash"),
            status_path_filters: string_vec_at(payload, "/status/path_filters"),
            status_language_filters: string_vec_at(payload, "/status/language_filters"),
        }
    }

    fn matches_without_checkpoint(&self, expected: &IndexExpectation) -> bool {
        self.scope_scope_id == expected.scope_id
            && self.scope_repository_id == expected.repository_id
            && self.scope_alias == expected.alias
            && self.scope_requested_ref == expected.requested_ref
            && self.scope_resolved_commit_sha == expected.resolved_commit_sha
            && self.scope_tree_hash == expected.tree_hash
            && self.scope_indexed_file_count == expected.selected_file_count
            && self.scope_path_filters == expected.path_filters
            && self.scope_language_filters == expected.language_filters
            && self.task_repository_id == expected.repository_id
            && self.task_alias == expected.alias
            && self.task_ref_selector == expected.requested_ref
            && self.task_resolved_commit_sha == expected.resolved_commit_sha
            && self.task_tree_hash == expected.tree_hash
            && self.task_source_scope == expected.scope_id
            && self.task_path_filters == expected.path_filters
            && self.task_language_filters == expected.language_filters
            && self.summary_repository_id == expected.repository_id
            && self.summary_source_scope == expected.scope_id
            && self.summary_resolved_commit_sha == expected.resolved_commit_sha
            && self.summary_tree_hash == expected.tree_hash
            && self.summary_indexed_file_count == expected.selected_file_count
            && self.status_repository_id == expected.repository_id
            && self.status_alias == expected.alias
            && self.status_last_indexed_scope_id == expected.scope_id
            && self.status_last_indexed_commit == expected.resolved_commit_sha
            && self.status_tree_hash == expected.tree_hash
            && self.status_path_filters == expected.path_filters
            && self.status_language_filters == expected.language_filters
    }

    fn checkpoint_matches(&self, expected: &IndexExpectation) -> bool {
        self.checkpoint_repository_id == expected.repository_id
            && self.checkpoint_source_scope == expected.scope_id
    }

    fn evidence(&self) -> Value {
        serde_json::json!({
            "scope": {
                "scope_id": self.scope_scope_id,
                "repository_id": self.scope_repository_id,
                "alias": self.scope_alias,
                "requested_ref": self.scope_requested_ref,
                "resolved_commit_sha": self.scope_resolved_commit_sha,
                "tree_hash": self.scope_tree_hash,
                "indexed_file_count": self.scope_indexed_file_count,
                "path_filters": self.scope_path_filters,
                "language_filters": self.scope_language_filters,
            },
            "task": {
                "repository_id": self.task_repository_id,
                "alias": self.task_alias,
                "ref_selector": self.task_ref_selector,
                "resolved_commit_sha": self.task_resolved_commit_sha,
                "tree_hash": self.task_tree_hash,
                "source_scope": self.task_source_scope,
                "path_filters": self.task_path_filters,
                "language_filters": self.task_language_filters,
                "incremental_base_ref": self.task_incremental_base_ref,
                "incremental_head_ref": self.task_incremental_head_ref,
            },
            "summary": {
                "repository_id": self.summary_repository_id,
                "source_scope": self.summary_source_scope,
                "base_resolved_commit_sha": self.summary_base_resolved_commit_sha,
                "resolved_commit_sha": self.summary_resolved_commit_sha,
                "tree_hash": self.summary_tree_hash,
                "indexed_file_count": self.summary_indexed_file_count,
            },
            "checkpoint": {
                "repository_id": self.checkpoint_repository_id,
                "source_scope": self.checkpoint_source_scope,
            },
            "status": {
                "repository_id": self.status_repository_id,
                "alias": self.status_alias,
                "last_indexed_scope_id": self.status_last_indexed_scope_id,
                "last_indexed_commit": self.status_last_indexed_commit,
                "tree_hash": self.status_tree_hash,
                "path_filters": self.status_path_filters,
                "language_filters": self.status_language_filters,
            },
        })
    }
}

pub(super) fn observed_git_file_count(
    runtime: &EvalRuntime,
    repo_name: &str,
    config: &Value,
    repository_path: &Path,
    ref_selector: &str,
) -> (Value, CommandResult) {
    let command = vec![
        "git".to_owned(),
        "-C".to_owned(),
        repository_path.display().to_string(),
        "ls-tree".to_owned(),
        "-r".to_owned(),
        "-z".to_owned(),
        "--name-only".to_owned(),
        ref_selector.to_owned(),
    ];
    let mut env = runtime.env.clone();
    env.insert("GIT_OPTIONAL_LOCKS".to_owned(), "0".to_owned());
    env.insert("LC_ALL".to_owned(), "C".to_owned());
    let mut result = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{repo_name}_observed_git_file_count"),
            command,
            &runtime.workspace,
            Some(env),
            git_file_count_timeout_seconds(runtime.timeout),
        ),
    );
    if !result.passed() {
        if plain_filesystem_source(repository_path, &result) {
            let git_diagnostic = result.stderr.trim().to_owned();
            result.exit_code = 0;
            result.stdout = serde_json::json!({
                "ref": ref_selector,
                "source_kind": "filesystem",
                "observed_git_file_count": Value::Null,
                "git_diagnostic": git_diagnostic,
            })
            .to_string();
            result.stderr.clear();
        }
        return (config.clone(), result);
    }
    let observed = result
        .stdout
        .as_bytes()
        .iter()
        .filter(|byte| **byte == 0)
        .count();
    let mut effective = config.clone();
    if let Some(object) = effective.as_object_mut() {
        object.insert(
            "observed_git_file_count".to_owned(),
            Value::from(observed as u64),
        );
    }
    result.stdout = serde_json::json!({
        "ref": ref_selector,
        "source_kind": "git",
        "observed_git_file_count": observed,
    })
    .to_string();
    (effective, result)
}

pub(super) fn git_file_count_timeout_seconds(runtime_timeout_seconds: u64) -> u64 {
    runtime_timeout_seconds.clamp(1, GIT_FILE_COUNT_TIMEOUT_SECONDS)
}

fn plain_filesystem_source(repository_path: &Path, result: &CommandResult) -> bool {
    result.exit_code != 124
        && result
            .stderr
            .to_ascii_lowercase()
            .contains("not a git repository")
        && !path_or_parent_has_git_metadata(repository_path)
}

fn path_or_parent_has_git_metadata(path: &Path) -> bool {
    let Ok(mut current) = path.canonicalize() else {
        return true;
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        match fs::symlink_metadata(current.join(".git")) {
            Ok(_) => return true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return true,
        }
        if !current.pop() {
            return false;
        }
    }
}

pub(super) fn scope_preview_command(binary: &Path, alias: &str, ref_selector: &str) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "scope".to_owned(),
        "preview".to_owned(),
        alias.to_owned(),
        "--ref".to_owned(),
        ref_selector.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn validation_result(name: String, contract: &str, passed: bool, evidence: Value) -> CommandResult {
    CommandResult {
        name,
        command: vec!["validate".to_owned(), contract.to_owned()],
        exit_code: i32::from(!passed),
        duration_ms: 0,
        stdout: if passed {
            evidence.to_string()
        } else {
            String::new()
        },
        stderr: if passed {
            String::new()
        } else {
            format!("{contract} evidence failed: {evidence}")
        },
    }
}

fn str_at<'a>(payload: &'a Value, pointer: &str) -> Option<&'a str> {
    payload.pointer(pointer).and_then(Value::as_str)
}

fn string_at(payload: &Value, pointer: &str) -> String {
    str_at(payload, pointer).unwrap_or_default().to_owned()
}

fn optional_u64_at(payload: &Value, pointer: &str) -> Option<u64> {
    payload.pointer(pointer).and_then(Value::as_u64)
}

fn u64_at(payload: &Value, pointer: &str) -> u64 {
    optional_u64_at(payload, pointer).unwrap_or_default()
}

fn string_vec_at(payload: &Value, pointer: &str) -> Vec<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "expectation_tests.rs"]
mod tests;
