// Direct tests for bounded index planning.

use super::*;

use crate::code::{
    source::{reset_source_read_counts_for_root, source_read_counts_for_root},
    test_fixtures::TempGitRepo,
};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn parser_worker_count_keeps_tiny_batches_serial() {
    assert_eq!(worker_count(7, 32 * 1024), 1);
}

#[test]
fn parser_worker_count_scales_with_bounded_batch_work() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(96, 4 * 1024 * 1024);

    assert_eq!(workers, available.min(8).min(96));
    assert!(workers >= 1);
}

#[test]
fn parser_worker_count_caps_thread_fanout_for_small_byte_batches() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(40, 128 * 1024);

    assert_eq!(workers, available.min(3).min(40));
}

#[test]
fn row_budget_overflow_reuses_one_fetched_parse_group() {
    let repo = TempGitRepo::create("row-budget-parse-overflow");
    let expected_paths = (0..5)
        .map(|index| format!("src/file_{index}.rs"))
        .collect::<Vec<_>>();
    for (index, path) in expected_paths.iter().enumerate() {
        repo.write(
            path,
            &format!(
                "pub fn value_{index}() -> usize {{ {index} }}\npub fn call_{index}() -> usize {{ value_{index}() }}\n"
            ),
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "dense row fixture"]);
    let budget =
        CodeIndexResourceBudget::new(128, 1024 * 1024, 1).expect("row budget should validate");
    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget)
        .expect("plan should prepare");
    reset_source_read_counts_for_root(repo.path.clone());

    let (mut plan, first_batch) = plan.parse_next_batch().expect("first batch should parse");
    let first_batch = first_batch.expect("first batch should exist");
    assert_eq!(plan.cursor, expected_paths.len());
    assert_eq!(plan.parsed_overflow.len(), expected_paths.len() - 1);
    assert_eq!(source_read_counts_for_root(&repo.path), (0, 1));
    assert_eq!(first_batch.files.len(), 1);
    assert!(
        first_batch.row_count() > budget.max_rows_per_batch,
        "one file remains indivisible when its facts exceed the row budget"
    );

    let mut parsed_paths = vec![first_batch.files[0].path.clone()];
    loop {
        let (next_plan, batch) = plan.parse_next_batch().expect("batch should parse");
        plan = next_plan;
        let Some(batch) = batch else {
            break;
        };
        assert_eq!(batch.files.len(), 1);
        assert!(batch.row_count() > budget.max_rows_per_batch);
        parsed_paths.push(batch.files[0].path.clone());
    }

    assert_eq!(parsed_paths, expected_paths);
    assert_eq!(source_read_counts_for_root(&repo.path), (0, 1));
}

#[test]
fn durable_partial_checkpoint_resumes_at_next_uncommitted_path_and_batch() {
    let (_repo, plan) = three_file_resume_plan("partial-checkpoint-resume");
    let checkpoint = checkpoint_for_plan(&plan, "indexing", 2, 2);

    let resumed = plan
        .resume_from_checkpoint(&checkpoint)
        .expect("committed prefix should resume");

    assert_eq!(resumed.cursor, 2);
    assert_eq!(resumed.next_batch_index, 3);
    assert!(resumed.parsed_overflow.is_empty());
    let (_, batch) = resumed
        .parse_next_batch()
        .expect("remaining path should parse");
    let batch = batch.expect("remaining batch should exist");
    assert_eq!(batch.batch_index, 3);
    assert_eq!(batch.files[0].path, "src/file_2.rs");
}

#[test]
fn durable_completed_checkpoint_resumes_as_a_fully_consumed_plan() {
    let (_repo, plan) = three_file_resume_plan("completed-checkpoint-resume");
    let checkpoint = checkpoint_for_plan(&plan, "completed", 3, 3);

    let resumed = plan
        .resume_from_checkpoint(&checkpoint)
        .expect("completed prefix should validate");
    let (resumed, batch) = resumed
        .parse_next_batch()
        .expect("completed plan should remain valid");

    assert!(batch.is_none());
    assert_eq!(resumed.cursor, 3);
    assert_eq!(resumed.next_batch_index, 4);
    assert!(resumed.parsed_overflow.is_empty());
}

#[test]
fn durable_query_index_repair_resumes_as_a_fully_consumed_plan() {
    let (_repo, plan) = three_file_resume_plan("repair-checkpoint-resume");
    let state = crate::domain::code_query_index_repair_state(
        16,
        crate::domain::CodeQueryIndexRepairResumePhase::ResolveImports,
    )
    .expect("repair checkpoint should format");
    let checkpoint = checkpoint_for_plan(&plan, &state, 3, 3);

    let resumed = plan
        .resume_from_checkpoint(&checkpoint)
        .expect("repair checkpoint complete prefix should validate");
    let (resumed, batch) = resumed
        .parse_next_batch()
        .expect("repair plan should remain fully consumed");

    assert!(batch.is_none());
    assert_eq!(resumed.cursor, 3);
    assert_eq!(resumed.next_batch_index, 4);
}

#[test]
fn durable_reference_resolution_direct_and_nested_reopen_without_parsing_a_blob() {
    let (_repo, plan) = three_file_resume_plan("reference-resolution-checkpoint-resume");
    let direct = crate::domain::code_reference_resolution_state(7, 31, Some("reference:31"))
        .expect("reference-resolution cursor should format");
    let parsed = crate::domain::code_reference_resolution(&direct)
        .expect("reference-resolution cursor should parse");
    let nested = crate::domain::code_reference_resolution_query_index_repair_state(16, parsed)
        .expect("nested repair cursor should format");

    for state in [direct, nested] {
        let checkpoint = checkpoint_for_plan(&plan, &state, 3, 3);
        let (resumed, batch) = plan
            .clone()
            .resume_from_checkpoint(&checkpoint)
            .expect("durable reference page should validate as a complete parser prefix")
            .parse_next_batch()
            .expect("reopened plan should not fetch or parse a blob");
        assert!(batch.is_none(), "state={state}");
        assert_eq!(resumed.cursor, 3, "state={state}");
    }
}

#[test]
fn completed_content_equivalent_commit_is_classified_as_a_zero_progress_restart() {
    let (_repo, plan) = three_file_resume_plan("content-equivalent-restart");
    let mut completed = checkpoint_for_plan(&plan, "completed", 3, 3);
    completed.resolved_commit_sha = "previous-commit-with-the-same-tree".to_owned();

    let direct_resume_error = plan
        .clone()
        .resume_from_checkpoint(&completed)
        .expect_err("a commit alias must not reuse the completed cursor");
    assert!(matches!(direct_resume_error, CodeIndexError::Invariant(_)));
    let restart = match plan
        .recover_from_checkpoint(&completed)
        .expect("a completed content-equivalent checkpoint should be recoverable")
    {
        CodeIndexPlanRecovery::ContentEquivalentRestart(plan) => plan,
        CodeIndexPlanRecovery::Resume(_) => panic!("a different commit must restart"),
    };
    let fresh_checkpoint = checkpoint_for_plan(&restart, "indexing", 0, 0);
    let resumed = restart
        .resume_from_content_equivalent_restart_checkpoint(&fresh_checkpoint)
        .expect("storage's fresh restart checkpoint should bind to the new commit");

    assert_eq!(resumed.cursor, 0);
    assert_eq!(resumed.next_batch_index, 1);
    assert!(resumed.parsed_overflow.is_empty());
}

#[test]
fn in_progress_commit_mismatch_remains_a_checkpoint_invariant() {
    let (_repo, plan) = three_file_resume_plan("in-progress-commit-mismatch");
    let mut checkpoint = checkpoint_for_plan(&plan, "indexing", 2, 2);
    checkpoint.resolved_commit_sha = "different-commit".to_owned();

    let error = plan
        .recover_from_checkpoint(&checkpoint)
        .expect_err("partial progress from another commit must not restart");

    assert!(matches!(error, CodeIndexError::Invariant(_)));
    assert!(error.to_string().contains("resolved commit"));
}

#[test]
fn durable_checkpoint_resume_rejects_invalid_identity_state_and_progress_table() {
    let (_repo, plan) = three_file_resume_plan("invalid-checkpoint-resume");
    let baseline = checkpoint_for_plan(&plan, "indexing", 2, 2);
    let mutate = |operation: fn(&mut CodeIndexCheckpoint)| {
        let mut checkpoint = baseline.clone();
        operation(&mut checkpoint);
        checkpoint
    };
    let invalid = vec![
        (
            "repository",
            mutate(|value| value.repository_id = "other".to_owned()),
        ),
        (
            "scope",
            mutate(|value| value.source_scope = "other".to_owned()),
        ),
        (
            "commit",
            mutate(|value| value.resolved_commit_sha = "other".to_owned()),
        ),
        ("tree", mutate(|value| value.tree_hash = "other".to_owned())),
        (
            "paths",
            mutate(|value| value.path_filters.push("other".to_owned())),
        ),
        ("languages", mutate(|value| value.language_filters.clear())),
        ("total", mutate(|value| value.total_path_count += 1)),
        (
            "budget",
            mutate(|value| value.resource_budget.max_files_per_batch += 1),
        ),
        (
            "state",
            mutate(|value| value.state = "finalizing:unknown".to_owned()),
        ),
        ("parsed", mutate(|value| value.parsed_file_count -= 1)),
        (
            "bounds",
            mutate(|value| {
                value.parsed_file_count = 4;
                value.committed_file_count = 4;
            }),
        ),
        (
            "last-path",
            mutate(|value| value.last_path = Some("src/file_0.rs".to_owned())),
        ),
        ("batch-count", mutate(|value| value.batch_count = 0)),
        (
            "partial-completed",
            mutate(|value| value.state = "completed".to_owned()),
        ),
        (
            "empty-prefix",
            mutate(|value| {
                value.parsed_file_count = 0;
                value.committed_file_count = 0;
            }),
        ),
    ];

    for (name, checkpoint) in invalid {
        let error = plan
            .clone()
            .resume_from_checkpoint(&checkpoint)
            .expect_err(name);
        assert!(matches!(&error, CodeIndexError::Invariant(_)));
        assert!(
            error
                .to_string()
                .contains("invalid code index resume checkpoint")
        );
    }
}

#[test]
fn durable_checkpoint_resume_rejects_uncommitted_parsed_overflow() {
    let (repo, _) = three_file_resume_plan("overflow-checkpoint-resume");
    let row_budget =
        CodeIndexResourceBudget::new(128, 1024 * 1024, 1).expect("row budget should validate");
    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), row_budget)
        .expect("overflow plan should prepare");
    let (plan, batch) = plan.parse_next_batch().expect("first batch should parse");
    assert!(batch.is_some());
    assert!(!plan.parsed_overflow.is_empty());
    let checkpoint = checkpoint_for_plan(&plan, "indexing", 1, 1);

    let error = plan
        .resume_from_checkpoint(&checkpoint)
        .expect_err("uncommitted overflow must never be skipped");

    assert!(error.to_string().contains("uncommitted parsed overflow"));
}

fn three_file_resume_plan(name: &str) -> (TempGitRepo, CodeIndexPlan) {
    let repo = TempGitRepo::create(name);
    for index in 0..3 {
        repo.write(
            &format!("src/file_{index}.rs"),
            &format!(
                "pub fn value_{index}() -> usize {{ {index} }}\npub fn call_{index}() -> usize {{ value_{index}() }}\n"
            ),
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "resume fixture"]);
    let budget =
        CodeIndexResourceBudget::new(1, 1024 * 1024, 10_000).expect("batch budget should validate");
    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget)
        .expect("resume plan should prepare");

    (repo, plan)
}

fn checkpoint_for_plan(
    plan: &CodeIndexPlan,
    state: &str,
    committed_file_count: usize,
    batch_count: usize,
) -> CodeIndexCheckpoint {
    let session = plan.session();
    CodeIndexCheckpoint {
        repository_id: session.repository_id,
        source_scope: session.source_scope,
        resolved_commit_sha: session.resolved_commit_sha,
        tree_hash: session.tree_hash,
        path_filters: session.path_filters,
        language_filters: session.language_filters,
        state: state.to_owned(),
        total_path_count: session.total_path_count,
        parsed_file_count: committed_file_count,
        committed_file_count,
        committed_symbol_count: 0,
        committed_reference_count: 0,
        committed_chunk_count: 0,
        committed_fact_row_count: committed_file_count,
        incremental_summary: None,
        batch_count,
        last_path: committed_file_count
            .checked_sub(1)
            .and_then(|index| plan.paths.get(index))
            .map(|entry| entry.path.clone()),
        resource_budget: session.resource_budget,
        updated_at_ms: 1,
    }
}

#[test]
fn batch_row_count_includes_feature_flags() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    build.feature_flags = crate::code::feature_flags::extract_feature_flags(
        crate::code::feature_flags::FeatureFlagFileInput {
            repository_id: &build.repository_id,
            source_scope: &build.source_scope,
            file_id: "file",
            path: "src/lib.rs",
            language_id: "rust",
            content: "if env::var(\"CHECKOUT_V2\").is_ok() && env::var(\"PAYMENTS_V2\").is_ok() {}",
            config_facts: &[],
        },
    )
    .expect("feature flags should extract");

    assert_eq!(batch_row_count(&build), 2);
}

#[test]
fn enabled_workspace_plan_binds_session_batches_and_nested_facts_to_one_scope() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("relay-plan-workspace-scope-{suffix}"));
    std::fs::create_dir_all(root.join("src")).expect("fixture directory should create");
    std::fs::write(root.join("src/a.rs"), "pub fn alpha() {}\n")
        .expect("first fixture should write");
    std::fs::write(root.join("src/b.rs"), "pub fn beta() { alpha(); }\n")
        .expect("second fixture should write");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        root.to_string_lossy(),
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let budget = CodeIndexResourceBudget::new(1, 1_024 * 1_024, 10_000)
        .expect("batch budget should validate");
    let mut plan = prepare_full_index_plan_with_workspace_detection(
        registration,
        selector,
        budget,
        &CodeWorkspaceDetectionConfig::enabled_all(),
    )
    .expect("enabled plan should prepare");
    let session_scope = plan.session().source_scope;
    assert!(session_scope.contains(":workspace-v1:"));
    let mut batch_count = 0usize;
    loop {
        let (next, batch) = plan.parse_next_batch().expect("batch should parse");
        plan = next;
        let Some(batch) = batch else {
            break;
        };
        batch_count += 1;
        assert_eq!(batch.source_scope, session_scope);
        for fact_scope in batch
            .files
            .iter()
            .map(|record| record.source_scope.as_str())
            .chain(
                batch
                    .symbols
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .references
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .imports
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .dependencies
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .feature_flags
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .routes
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .chunks
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
            .chain(
                batch
                    .diagnostics
                    .iter()
                    .map(|record| record.source_scope.as_str()),
            )
        {
            assert_eq!(fact_scope, session_scope);
        }
    }
    assert_eq!(batch_count, 2, "one-file budget should produce two batches");
    std::fs::remove_dir_all(root).expect("fixture directory should clean up");
}
