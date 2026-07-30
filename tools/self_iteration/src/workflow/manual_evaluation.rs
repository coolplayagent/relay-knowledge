use crate::{candidate_git, config::Config, history};

use super::{
    candidate_evaluation::evaluate_candidate_for_patch, output::print_score,
    persistence::persist_scored_run, run_identity::new_manual_evaluate_run_id,
};

pub(super) fn run_evaluate(config: &Config, paths: &history::HistoryPaths) -> Result<i32, String> {
    let run_id = new_manual_evaluate_run_id();
    let patch = candidate_git::capture_patch(&config.workspace, paths, &run_id, "HEAD")?;
    let evaluation = evaluate_candidate_for_patch(config, paths, &run_id, &patch)?;
    let record = persist_scored_run(config, paths, &run_id, &patch, None, &evaluation, None)?;
    print_score(&record);
    Ok(if record["score"].as_f64().unwrap_or(0.0) > 0.0 {
        0
    } else {
        1
    })
}
