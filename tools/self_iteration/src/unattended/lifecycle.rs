pub fn run_unattended_layered_loop(
    config: &Config,
    paths: &history::HistoryPaths,
) -> Result<i32, String> {
    if !config.use_current_candidate {
        git_ops::ensure_clean_worktree(&config.workspace)?;
    }
    let cases_config =
        cases::load_cases(&config.workspace.join("tools/self_iteration/cases.json"))?;
    let mut state = load_unattended_state(paths)?;
    let mut iteration = 0usize;
    loop {
        let now = unix_timestamp();
        if let Some(reason) = unattended_stop_reason(config, &state, now) {
            state.completed = true;
            state.completion_reason = Some(reason.clone());
            save_unattended_state(paths, &state)?;
            println!("[self-iterate] unattended-layered stopped: {reason}");
            return Ok(0);
        }
        if config.max_iterations.is_some_and(|max| iteration >= max) {
            state.completed = true;
            state.completion_reason = Some("max iterations reached".to_owned());
            save_unattended_state(paths, &state)?;
            return Ok(0);
        }
        iteration += 1;
        state.cycle_count += 1;
        println!(
            "[self-iterate] unattended-layered cycle={} accepted={} elapsed_s={}",
            state.cycle_count,
            state.accepted_count,
            state.elapsed_seconds(now)
        );
        let outcome = run_unattended_cycle(config, paths, &cases_config, &mut state)?;
        state.last_updated_at = unix_timestamp();
        save_unattended_state(paths, &state)?;
        maybe_run_deep_check(config, paths, &mut state)?;
        let sleep_seconds = unattended_sleep_seconds(config, outcome);
        if sleep_seconds > 0 && !config.dry_run_codex {
            git_ops::sleep_seconds(sleep_seconds);
        }
    }
}
