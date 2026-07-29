fn run_unattended_cycle(
    config: &Config,
    paths: &history::HistoryPaths,
    cases_config: &serde_json::Value,
    state: &mut UnattendedState,
) -> Result<LayeredCycleOutcome, String> {
    if config.use_current_candidate {
        return run_current_candidate_cycle(config, paths, state);
    }
    let macro_trigger = macro_trigger(config, paths, state)?;
    let kind = if macro_trigger.is_some() {
        LayerAttemptKind::MacroExplore
    } else {
        LayerAttemptKind::Explore
    };
    let attempts = if kind.is_macro() {
        1
    } else {
        config.max_explore_attempts_per_cycle
    };
    let mut last_outcome = LayeredCycleOutcome::EmptyCandidate;
    for attempt in 0..attempts {
        let category = if kind.is_macro() {
            EvaluationCategory::Competitive
        } else {
            next_unattended_category(state)
        };
        let outcome = run_unattended_attempt(UnattendedAttemptInput {
            config,
            paths,
            cases_config,
            state,
            kind,
            category,
            attempt_index: attempt + 1,
            macro_trigger: macro_trigger.as_deref(),
        })?;
        last_outcome = outcome;
        if !outcome.should_retry_explore() {
            break;
        }
    }
    Ok(last_outcome)
}

fn run_current_candidate_cycle(
    config: &Config,
    paths: &history::HistoryPaths,
    state: &mut UnattendedState,
) -> Result<LayeredCycleOutcome, String> {
    let category = selected_or_default_category(config);
    let run_id = new_layer_run_id("current-candidate");
    let base_ref = candidate_git::current_head(&config.workspace)?;
    let patch = candidate_git::capture_patch(&config.workspace, paths, &run_id, &base_ref)?;
    if !patch.has_diff() {
        let current_config = unattended_config(config, "smoke", category, 1);
        let metadata = unattended_metadata(
            config,
            state,
            "current_candidate",
            category,
            MetadataLinks {
                promotion_decision: Some("empty_candidate"),
                ..MetadataLinks::default()
            },
        );
        persist_empty_candidate(&current_config, paths, &run_id, &patch, None, &metadata)?;
        state.consecutive_empty_candidates += 1;
        return Ok(LayeredCycleOutcome::EmptyCandidate);
    }
    state.consecutive_empty_candidates = 0;
    let screen_config = unattended_config(config, "smoke", category, 1);
    let screen_record = evaluate_unattended_layer(UnattendedEvaluationInput {
        config: &screen_config,
        paths,
        run_id: &new_layer_run_id("current-screen"),
        patch: &patch,
        codex: None,
        metadata: unattended_metadata(
            config,
            state,
            "screen",
            category,
            MetadataLinks {
                parent_run_id: Some(&run_id),
                ..MetadataLinks::default()
            },
        ),
        commit: false,
        base_ref: &base_ref,
    })?;
    if !score_accepted(&screen_record) {
        update_unattended_rejection_counters(state, category);
        candidate_git::reject_candidate(&config.workspace, &patch, false)?;
        return Ok(LayeredCycleOutcome::Rejected);
    }
    let validate_config = unattended_config(config, "fast", category, 1);
    let validate_record = evaluate_unattended_layer(UnattendedEvaluationInput {
        config: &validate_config,
        paths,
        run_id: &new_layer_run_id("current-validate"),
        patch: &patch,
        codex: None,
        metadata: unattended_metadata(
            config,
            state,
            "validate",
            category,
            MetadataLinks {
                parent_run_id: Some(&run_id),
                promoted_from_run_id: screen_record
                    .get("run_id")
                    .and_then(serde_json::Value::as_str),
                ..MetadataLinks::default()
            },
        ),
        commit: true,
        base_ref: &base_ref,
    })?;
    if validate_record["accepted"].as_bool().unwrap_or(false) {
        state.accepted_count += 1;
        state.consecutive_promotion_failures = 0;
        if category == EvaluationCategory::Competitive {
            state.competitive_promotion_failures = 0;
        }
        return Ok(LayeredCycleOutcome::Accepted);
    }
    update_unattended_rejection_counters(state, category);
    candidate_git::reject_candidate(&config.workspace, &patch, false)?;
    Ok(LayeredCycleOutcome::Rejected)
}
