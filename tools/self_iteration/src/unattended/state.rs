fn load_unattended_state(paths: &history::HistoryPaths) -> Result<UnattendedState, String> {
    if !paths.unattended_state.exists() {
        return Ok(UnattendedState::new(unix_timestamp()));
    }
    let text = std::fs::read_to_string(&paths.unattended_state).map_err(|error| {
        format!(
            "failed to read {}: {error}",
            paths.unattended_state.display()
        )
    })?;
    let state = serde_json::from_str::<UnattendedState>(&text).map_err(|error| {
        format!(
            "failed to parse {}: {error}",
            paths.unattended_state.display()
        )
    })?;
    if state.completed || state.strategy != Strategy::UnattendedLayered.label() {
        Ok(UnattendedState::new(unix_timestamp()))
    } else {
        Ok(state)
    }
}

fn save_unattended_state(
    paths: &history::HistoryPaths,
    state: &UnattendedState,
) -> Result<(), String> {
    paths.ensure()?;
    std::fs::write(
        &paths.unattended_state,
        serde_json::to_string_pretty(state).map_err(|error| error.to_string())? + "\n",
    )
    .map_err(|error| {
        format!(
            "failed to write {}: {error}",
            paths.unattended_state.display()
        )
    })
}

fn unattended_stop_reason(config: &Config, state: &UnattendedState, now: u64) -> Option<String> {
    if config
        .stop_after_accepted
        .unwrap_or(UNATTENDED_ACCEPT_LIMIT)
        <= state.accepted_count
    {
        return Some("accepted limit reached".to_owned());
    }
    if state.elapsed_seconds(now) >= config.max_wall_clock_hours.saturating_mul(3600) {
        return Some("wall clock limit reached".to_owned());
    }
    if state.consecutive_empty_candidates >= config.max_consecutive_empty_candidates {
        return Some("consecutive empty candidate limit reached".to_owned());
    }
    if state.consecutive_promotion_failures >= config.max_consecutive_promotion_failures {
        return Some("consecutive promotion failure limit reached".to_owned());
    }
    None
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
