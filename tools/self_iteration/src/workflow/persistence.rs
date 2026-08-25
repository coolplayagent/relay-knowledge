use crate::{candidate_git::PatchSnapshot, codex, config::Config, evaluator, history, scoring};

use super::{
    report_metadata::{
        comparison_baseline, optimization_plan, patch_metadata, selected_categories_value,
    },
    run_identity::unix_timestamp_string,
};

pub(super) fn persist_scored_run(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &PatchSnapshot,
    codex: Option<&codex::CodexResult>,
    evaluation: &evaluator::EvaluationRun,
    commit: Option<&str>,
) -> Result<serde_json::Value, String> {
    let category_focus = config.category_focus_key();
    let previous = history::previous_scored_run_for_workload(
        paths,
        &config.profile,
        category_focus.as_deref(),
    )?;
    let profile_best_accepted = history::best_accepted_run_for_profile(paths, &config.profile)?;
    let score = scoring::score_evaluation(
        &evaluation.observation,
        scoring::ScoreBaselines {
            workload_previous: previous.as_ref(),
            profile_best_accepted: profile_best_accepted.as_ref(),
        },
    );
    persist_scored_run_with_score(PersistInput {
        config,
        paths,
        run_id,
        patch,
        codex,
        evaluation,
        commit,
        score: &score,
        previous_run: previous.as_ref(),
        metadata: None,
    })
}

pub(crate) struct PersistInput<'a> {
    pub(crate) config: &'a Config,
    pub(crate) paths: &'a history::HistoryPaths,
    pub(crate) run_id: &'a str,
    pub(crate) patch: &'a PatchSnapshot,
    pub(crate) codex: Option<&'a codex::CodexResult>,
    pub(crate) evaluation: &'a evaluator::EvaluationRun,
    pub(crate) commit: Option<&'a str>,
    pub(crate) score: &'a scoring::ScoreBreakdown,
    pub(crate) previous_run: Option<&'a serde_json::Value>,
    pub(crate) metadata: Option<&'a serde_json::Value>,
}

pub(crate) fn persist_scored_run_with_score(
    input: PersistInput<'_>,
) -> Result<serde_json::Value, String> {
    let timestamp = unix_timestamp_string();
    let patch = patch_metadata(input.patch);
    let optimization_plan = optimization_plan(input.patch, input.score, input.codex);
    let category_focus = input.config.category_focus_key();
    let selected_categories = input.config.selected_category_labels();
    let selected_categories_report = selected_categories_value(&selected_categories);
    let comparison_baseline = comparison_baseline(
        input.paths,
        &input.config.profile,
        category_focus.as_deref(),
        input.previous_run,
    )?;
    let report = serde_json::json!({
        "run_id": input.run_id,
        "profile": input.config.profile,
        "product_binary_profile": input.config.product_binary_profile().map(|profile| profile.as_str()),
        "product_binary_path": input.config.product_binary_path().map(|path| path.display().to_string()),
        "strategy": input.config.strategy.label(),
        "category_focus": category_focus.as_deref(),
        "selected_categories": selected_categories_report,
        "unattended": input.metadata,
        "workspace": input.config.workspace.display().to_string(),
        "patch": patch,
        "optimization_plan": optimization_plan,
        "comparison_baseline": comparison_baseline,
        "score_accepted": input.score.accepted,
        "committed": input.commit.is_some(),
        "adoption_status": if input.commit.is_some() {
            "committed"
        } else if input.score.accepted {
            "would_accept"
        } else {
            "rejected"
        },
        "codex": input.codex.map(codex::CodexResult::serializable),
        "evaluation": input.evaluation.report,
        "score": input.score,
        "degradations": input.score.degradations,
        "improvements": input.score.improvements,
    });
    let report_path = history::write_report(input.paths, input.run_id, &report)?;
    let record = history::make_run_record(history::RunRecordInput {
        run_id: input.run_id,
        timestamp: &timestamp,
        profile: &input.config.profile,
        product_binary_profile: input.config.product_binary_profile(),
        category_focus: category_focus.as_deref(),
        selected_categories: &selected_categories,
        report_path: &report_path,
        commit: input.commit,
        score: input.score,
        observation: &input.evaluation.observation,
    });
    let mut record = record;
    if let Some(object) = record.as_object_mut() {
        object.insert("patch".to_owned(), patch);
        object.insert("optimization_plan".to_owned(), optimization_plan);
        object.insert("comparison_baseline".to_owned(), comparison_baseline);
        object.insert(
            "strategy".to_owned(),
            serde_json::json!(input.config.strategy.label()),
        );
        if let Some(metadata) = input.metadata.and_then(serde_json::Value::as_object) {
            for key in [
                "layer",
                "parent_run_id",
                "promoted_from_run_id",
                "macro_trigger",
                "promotion_decision",
                "wall_clock_started_at",
                "wall_clock_elapsed_seconds",
            ] {
                if let Some(value) = metadata.get(key) {
                    object.insert(key.to_owned(), value.clone());
                }
            }
        }
    }
    if !history::is_evaluate_run(&record) {
        history::memory::write_run_memory(input.paths, &record)?;
    }
    history::append_run(input.paths, &record)?;
    history::export_history(input.paths)?;
    Ok(record)
}
