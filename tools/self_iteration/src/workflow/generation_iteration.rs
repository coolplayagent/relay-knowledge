use crate::{
    candidate_git, codex, command,
    config::Config,
    evaluator, history,
    scoring::{self, EvaluationObservation, GateObservation},
};

use super::{
    adopted_documentation::write_adopted_optimization_document,
    candidate_evaluation::evaluate_candidate_for_patch,
    documentation_gate::apply_candidate_documentation_gate,
    output::print_score,
    persistence::{PersistInput, persist_scored_run, persist_scored_run_with_score},
    run_identity::new_run_id,
};

pub(super) fn run_generation_iteration(
    config: &Config,
    paths: &history::HistoryPaths,
) -> Result<bool, String> {
    let run_id = new_run_id();
    if !config.use_current_candidate {
        candidate_git::ensure_clean_worktree(&config.workspace)?;
    }
    let base_ref = candidate_git::current_head(&config.workspace)?;
    let codex_result = if config.use_current_candidate {
        println!("[self-iterate] using current working tree as candidate");
        None
    } else {
        let prompt = codex::build_prompt(
            paths,
            &config.workspace,
            &run_id,
            &config.profile,
            config.categories.as_ref(),
        );
        let result = codex::run_codex(config, &prompt);
        println!(
            "[self-iterate] codex exit={} duration_ms={}",
            result.exit_code, result.duration_ms
        );
        Some(result)
    };
    let patch = candidate_git::capture_patch(&config.workspace, paths, &run_id, &base_ref)?;
    if codex_result
        .as_ref()
        .is_some_and(|result| !result.succeeded())
    {
        let observation = EvaluationObservation {
            gates: vec![GateObservation {
                name: "codex_generation".to_owned(),
                passed: false,
                duration_ms: codex_result
                    .as_ref()
                    .map(|result| result.duration_ms)
                    .unwrap_or(0),
                message: codex_result
                    .as_ref()
                    .map(|result| command::last_output_line(&result.stdout, &result.stderr))
                    .unwrap_or_default(),
            }],
            cases: Vec::new(),
            metrics: Vec::new(),
            generated_diff: patch.has_diff(),
        };
        let evaluation = evaluator::EvaluationRun {
            observation,
            report: serde_json::json!({"generated_diff": patch.has_diff()}),
        };
        let record = persist_scored_run(
            config,
            paths,
            &run_id,
            &patch,
            codex_result.as_ref(),
            &evaluation,
            None,
        )?;
        candidate_git::reject_candidate(&config.workspace, &patch, !config.use_current_candidate)?;
        print_score(&record);
        return Ok(false);
    }
    if !patch.has_diff() {
        let evaluation = evaluator::EvaluationRun {
            observation: EvaluationObservation::empty(false),
            report: serde_json::json!({"generated_diff": false}),
        };
        let record = persist_scored_run(
            config,
            paths,
            &run_id,
            &patch,
            codex_result.as_ref(),
            &evaluation,
            None,
        )?;
        print_score(&record);
        return Ok(false);
    }
    println!("[self-iterate] candidate patch: {}", patch.path.display());
    let mut evaluation = evaluate_candidate_for_patch(config, paths, &run_id, &patch)?;
    apply_candidate_documentation_gate(&mut evaluation, &patch);
    let category_focus = config.category_focus_key();
    let previous_run = history::previous_scored_run_for_workload(
        paths,
        &config.profile,
        category_focus.as_deref(),
    )?;
    let profile_best_accepted = history::best_accepted_run_for_profile(paths, &config.profile)?;
    let score = scoring::score_evaluation(
        &evaluation.observation,
        scoring::ScoreBaselines {
            workload_previous: previous_run.as_ref(),
            profile_best_accepted: profile_best_accepted.as_ref(),
        },
    );
    let commit = if score.accepted {
        write_adopted_optimization_document(
            &config.workspace,
            &run_id,
            &patch,
            &score,
            &evaluation,
        )?;
        Some(candidate_git::commit_candidate(
            &config.workspace,
            config.commit_message.as_deref(),
            score.score,
            &base_ref,
        )?)
    } else {
        None
    };
    let record = persist_scored_run_with_score(PersistInput {
        config,
        paths,
        run_id: &run_id,
        patch: &patch,
        codex: codex_result.as_ref(),
        evaluation: &evaluation,
        commit: commit.as_deref(),
        score: &score,
        previous_run: previous_run.as_ref(),
        metadata: None,
    })?;
    if record["accepted"].as_bool().unwrap_or(false) {
        println!(
            "[self-iterate] accepted commit={}",
            commit.unwrap_or_default()
        );
        print_score(&record);
        Ok(true)
    } else {
        candidate_git::reject_candidate(&config.workspace, &patch, !config.use_current_candidate)?;
        println!("[self-iterate] rejected candidate and restored working tree");
        print_score(&record);
        Ok(false)
    }
}
