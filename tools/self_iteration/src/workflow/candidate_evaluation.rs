use crate::{candidate_git::PatchSnapshot, cases, config::Config, evaluator, history};

pub(crate) fn evaluate_candidate_for_patch(
    config: &Config,
    paths: &history::HistoryPaths,
    run_id: &str,
    patch: &PatchSnapshot,
) -> Result<evaluator::EvaluationRun, String> {
    let cases_config =
        cases::load_cases(&config.workspace.join("tools/self_iteration/cases.json"))?;
    evaluator::evaluate_candidate(
        config,
        paths,
        run_id,
        &cases_config,
        patch.has_diff(),
        &patch.diff,
    )
}
