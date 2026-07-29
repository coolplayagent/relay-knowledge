fn maybe_run_deep_check(
    config: &Config,
    paths: &history::HistoryPaths,
    state: &mut UnattendedState,
) -> Result<(), String> {
    if state.accepted_count == 0 {
        return Ok(());
    }
    let now = unix_timestamp();
    let accept_due = config.deep_check_interval_accepts > 0
        && state.accepted_count % config.deep_check_interval_accepts == 0;
    let time_due = now.saturating_sub(state.last_deep_check_at)
        >= config.deep_check_interval_hours.saturating_mul(3600);
    if !accept_due && !time_due {
        return Ok(());
    }
    state.last_deep_check_at = now;
    save_unattended_state(paths, state)?;
    let run_id = new_layer_run_id("deep-check");
    let patch = candidate_git::capture_patch(&config.workspace, paths, &run_id, "HEAD")?;
    let mut full_config = config.clone();
    full_config.profile = "full".to_owned();
    full_config.categories = None;
    let evaluation = evaluate_candidate_for_patch(&full_config, paths, &run_id, &patch)?;
    let metadata = serde_json::json!({
        "strategy": config.strategy.label(),
        "layer": "deep_check",
        "parent_run_id": serde_json::Value::Null,
        "promoted_from_run_id": serde_json::Value::Null,
        "macro_trigger": serde_json::Value::Null,
        "category_focus": serde_json::Value::Null,
        "promotion_decision": "risk_audit",
        "wall_clock_started_at": state.started_at.to_string(),
        "wall_clock_elapsed_seconds": state.elapsed_seconds(now),
    });
    let record = persist_scored_run_with_metadata(MetadataPersistInput {
        config: &full_config,
        paths,
        run_id: &run_id,
        patch: &patch,
        codex: None,
        evaluation: &evaluation,
        commit: None,
        metadata: &metadata,
    })?;
    print_score(&record);
    Ok(())
}
