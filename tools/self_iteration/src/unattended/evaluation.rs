struct UnattendedEvaluationInput<'a> {
    config: &'a Config,
    paths: &'a history::HistoryPaths,
    run_id: &'a str,
    patch: &'a candidate_git::PatchSnapshot,
    codex: Option<&'a codex::CodexResult>,
    metadata: serde_json::Value,
    commit: bool,
    base_ref: &'a str,
}

#[derive(Default)]
struct MetadataLinks<'a> {
    parent_run_id: Option<&'a str>,
    promoted_from_run_id: Option<&'a str>,
    macro_trigger: Option<&'a str>,
    promotion_decision: Option<&'a str>,
}

struct MetadataPersistInput<'a> {
    config: &'a Config,
    paths: &'a history::HistoryPaths,
    run_id: &'a str,
    patch: &'a candidate_git::PatchSnapshot,
    codex: Option<&'a codex::CodexResult>,
    evaluation: &'a evaluator::EvaluationRun,
    commit: Option<&'a str>,
    metadata: &'a serde_json::Value,
}

fn evaluate_unattended_layer(
    input: UnattendedEvaluationInput<'_>,
) -> Result<serde_json::Value, String> {
    let mut evaluation =
        evaluate_candidate_for_patch(input.config, input.paths, input.run_id, input.patch)?;
    apply_candidate_documentation_gate(&mut evaluation, input.patch);
    let category_focus = input.config.category_focus_key();
    let previous_run = history::previous_scored_run_for_workload(
        input.paths,
        &input.config.profile,
        category_focus.as_deref(),
    )?;
    let profile_best_accepted =
        history::best_accepted_run_for_profile(input.paths, &input.config.profile)?;
    let score = scoring::score_evaluation(
        &evaluation.observation,
        scoring::ScoreBaselines {
            workload_previous: previous_run.as_ref(),
            profile_best_accepted: profile_best_accepted.as_ref(),
        },
    );
    let commit = if input.commit && score.accepted {
        write_adopted_optimization_document(
            &input.config.workspace,
            input.run_id,
            input.patch,
            &score,
            &evaluation,
        )?;
        Some(candidate_git::commit_candidate(
            &input.config.workspace,
            input.config.commit_message.as_deref(),
            score.score,
            input.base_ref,
        )?)
    } else {
        None
    };
    let record = persist_scored_run_with_score(PersistInput {
        config: input.config,
        paths: input.paths,
        run_id: input.run_id,
        patch: input.patch,
        codex: input.codex,
        evaluation: &evaluation,
        commit: commit.as_deref(),
        score: &score,
        previous_run: previous_run.as_ref(),
        metadata: Some(&input.metadata),
    })?;
    print_score(&record);
    Ok(record)
}

fn persist_generation_failure(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &candidate_git::PatchSnapshot,
    codex_result: &codex::CodexResult,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    let observation = crate::scoring::EvaluationObservation {
        gates: vec![crate::scoring::GateObservation {
            name: "codex_generation".to_owned(),
            passed: false,
            duration_ms: codex_result.duration_ms,
            message: command::last_output_line(&codex_result.stdout, &codex_result.stderr),
        }],
        cases: Vec::new(),
        metrics: Vec::new(),
        generated_diff: patch.has_diff(),
    };
    let evaluation = evaluator::EvaluationRun {
        observation,
        report: serde_json::json!({"generated_diff": patch.has_diff()}),
    };
    let record = persist_scored_run_with_metadata(MetadataPersistInput {
        config,
        paths,
        run_id,
        patch,
        codex: Some(codex_result),
        evaluation: &evaluation,
        commit: None,
        metadata,
    })?;
    print_score(&record);
    Ok(())
}

fn persist_empty_candidate(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &candidate_git::PatchSnapshot,
    codex: Option<&codex::CodexResult>,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    let evaluation = evaluator::EvaluationRun {
        observation: crate::scoring::EvaluationObservation::empty(false),
        report: serde_json::json!({"generated_diff": false}),
    };
    let record = persist_scored_run_with_metadata(MetadataPersistInput {
        config,
        paths,
        run_id,
        patch,
        codex,
        evaluation: &evaluation,
        commit: None,
        metadata,
    })?;
    print_score(&record);
    Ok(())
}

fn persist_scored_run_with_metadata(
    input: MetadataPersistInput<'_>,
) -> Result<serde_json::Value, String> {
    let category_focus = input.config.category_focus_key();
    let previous = history::previous_scored_run_for_workload(
        input.paths,
        &input.config.profile,
        category_focus.as_deref(),
    )?;
    let profile_best_accepted =
        history::best_accepted_run_for_profile(input.paths, &input.config.profile)?;
    let score = scoring::score_evaluation(
        &input.evaluation.observation,
        scoring::ScoreBaselines {
            workload_previous: previous.as_ref(),
            profile_best_accepted: profile_best_accepted.as_ref(),
        },
    );
    persist_scored_run_with_score(PersistInput {
        config: input.config,
        paths: input.paths,
        run_id: input.run_id,
        patch: input.patch,
        codex: input.codex,
        evaluation: input.evaluation,
        commit: input.commit,
        score: &score,
        previous_run: previous.as_ref(),
        metadata: Some(input.metadata),
    })
}
