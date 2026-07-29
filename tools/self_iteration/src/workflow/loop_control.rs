use crate::{
    candidate_git,
    config::{Config, Strategy},
    history, unattended,
};

use super::{generation_iteration::run_generation_iteration, pacing::sleep_seconds};

pub(super) fn run_loop(config: &Config, paths: &history::HistoryPaths) -> Result<i32, String> {
    if config.strategy == Strategy::UnattendedLayered {
        return unattended::run_unattended_layered_loop(config, paths);
    }
    if config.max_iterations == Some(0) || config.stop_after_accepted == Some(0) {
        return Ok(0);
    }
    if !config.use_current_candidate {
        candidate_git::ensure_clean_worktree(&config.workspace)?;
    }
    let mut iteration = 0usize;
    let mut accepted_count = 0usize;
    loop {
        if config.max_iterations.is_some_and(|max| iteration >= max) {
            return Ok(0);
        }
        if config
            .stop_after_accepted
            .is_some_and(|max| accepted_count >= max)
        {
            return Ok(0);
        }
        iteration += 1;
        println!("[self-iterate] iteration {iteration} starting");
        match run_generation_iteration(config, paths) {
            Ok(true) => accepted_count += 1,
            Ok(false) => {}
            Err(error) if config.fail_fast => return Err(error),
            Err(error) => {
                eprintln!("[self-iterate] iteration failed: {error}");
                if config.max_iterations.is_some_and(|max| iteration >= max) {
                    return Ok(1);
                }
            }
        }
        sleep_seconds(config.sleep_seconds);
    }
}
