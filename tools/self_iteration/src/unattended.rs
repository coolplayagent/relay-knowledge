use serde::{Deserialize, Serialize};

use crate::{
    PersistInput, apply_candidate_documentation_gate, cases, codex, command,
    config::{CategorySet, Config, EvaluationCategory, Strategy},
    evaluate_candidate_for_patch, evaluator, git_ops, history, new_layer_run_id, number,
    persist_scored_run_with_score, print_score, scoring, unix_timestamp,
    write_adopted_optimization_document,
};

const UNATTENDED_ACCEPT_LIMIT: usize = 8;
const COMPETITIVE_GAP_EPSILON: f64 = 0.02;
const CATEGORY_ROTATION: [EvaluationCategory; 4] = [
    EvaluationCategory::Competitive,
    EvaluationCategory::SemanticVector,
    EvaluationCategory::Performance,
    EvaluationCategory::RepositorySets,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnattendedState {
    strategy: String,
    started_at: u64,
    last_updated_at: u64,
    accepted_count: usize,
    cycle_count: usize,
    category_index: usize,
    consecutive_empty_candidates: usize,
    consecutive_promotion_failures: usize,
    competitive_promotion_failures: usize,
    last_deep_check_at: u64,
    completed: bool,
    completion_reason: Option<String>,
}

impl UnattendedState {
    fn new(now: u64) -> Self {
        Self {
            strategy: Strategy::UnattendedLayered.label().to_owned(),
            started_at: now,
            last_updated_at: now,
            accepted_count: 0,
            cycle_count: 0,
            category_index: 0,
            consecutive_empty_candidates: 0,
            consecutive_promotion_failures: 0,
            competitive_promotion_failures: 0,
            last_deep_check_at: now,
            completed: false,
            completion_reason: None,
        }
    }

    fn elapsed_seconds(&self, now: u64) -> u64 {
        now.saturating_sub(self.started_at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerAttemptKind {
    Explore,
    MacroExplore,
}

impl LayerAttemptKind {
    fn label(self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::MacroExplore => "macro_explore",
        }
    }

    fn timeout_seconds(self, config: &Config) -> u64 {
        match self {
            Self::Explore => config.explore_timeout_seconds,
            Self::MacroExplore => config.macro_explore_timeout_seconds,
        }
    }

    fn is_macro(self) -> bool {
        self == Self::MacroExplore
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayeredCycleOutcome {
    Accepted,
    Rejected,
    EmptyCandidate,
    CodexTimeout,
    CodexFailed,
}

impl LayeredCycleOutcome {
    fn should_retry_explore(self) -> bool {
        matches!(
            self,
            Self::EmptyCandidate | Self::CodexTimeout | Self::CodexFailed
        )
    }
}

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
    let base_ref = git_ops::current_head(&config.workspace)?;
    let patch = git_ops::capture_patch(&config.workspace, paths, &run_id, &base_ref)?;
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
        git_ops::reject_candidate(&config.workspace, &patch, false)?;
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
    git_ops::reject_candidate(&config.workspace, &patch, false)?;
    Ok(LayeredCycleOutcome::Rejected)
}

struct UnattendedAttemptInput<'a> {
    config: &'a Config,
    paths: &'a history::HistoryPaths,
    cases_config: &'a serde_json::Value,
    state: &'a mut UnattendedState,
    kind: LayerAttemptKind,
    category: EvaluationCategory,
    attempt_index: usize,
    macro_trigger: Option<&'a str>,
}

fn run_unattended_attempt(
    input: UnattendedAttemptInput<'_>,
) -> Result<LayeredCycleOutcome, String> {
    if !input.config.use_current_candidate {
        git_ops::ensure_clean_worktree(&input.config.workspace)?;
    }
    let parent_run_id = new_layer_run_id(input.kind.label());
    let base_ref = git_ops::current_head(&input.config.workspace)?;
    let explore_config = unattended_config(
        input.config,
        "smoke",
        input.category,
        input.kind.timeout_seconds(input.config),
    );
    let prompt = codex::build_unattended_prompt(
        input.paths,
        &input.config.workspace,
        &parent_run_id,
        &explore_config.profile,
        input.category,
        input.kind.is_macro(),
        input.cases_config,
    );
    let codex_result = codex::run_codex(&explore_config, &prompt);
    println!(
        "[self-iterate] unattended {} category={} attempt={} codex exit={} duration_ms={}",
        input.kind.label(),
        input.category.label(),
        input.attempt_index,
        codex_result.exit_code,
        codex_result.duration_ms
    );
    let patch = git_ops::capture_patch(
        &input.config.workspace,
        input.paths,
        &parent_run_id,
        &base_ref,
    )?;
    if !codex_result.succeeded() {
        let timed_out = codex_result.exit_code == 124;
        let metadata = unattended_metadata(
            input.config,
            input.state,
            input.kind.label(),
            input.category,
            MetadataLinks {
                macro_trigger: input.macro_trigger,
                promotion_decision: Some(if timed_out {
                    "codex_timeout"
                } else {
                    "codex_failed"
                }),
                ..MetadataLinks::default()
            },
        );
        persist_generation_failure(
            &explore_config,
            input.paths,
            &parent_run_id,
            &patch,
            &codex_result,
            &metadata,
        )?;
        git_ops::reject_candidate(&input.config.workspace, &patch, true)?;
        if timed_out {
            return Ok(LayeredCycleOutcome::CodexTimeout);
        }
        return Ok(LayeredCycleOutcome::CodexFailed);
    }
    if !patch.has_diff() {
        let metadata = unattended_metadata(
            input.config,
            input.state,
            input.kind.label(),
            input.category,
            MetadataLinks {
                macro_trigger: input.macro_trigger,
                promotion_decision: Some("empty_candidate"),
                ..MetadataLinks::default()
            },
        );
        persist_empty_candidate(
            &explore_config,
            input.paths,
            &parent_run_id,
            &patch,
            Some(&codex_result),
            &metadata,
        )?;
        input.state.consecutive_empty_candidates += 1;
        return Ok(LayeredCycleOutcome::EmptyCandidate);
    }
    input.state.consecutive_empty_candidates = 0;
    println!(
        "[self-iterate] unattended candidate patch: {}",
        patch.path.display()
    );
    let screen_record = evaluate_unattended_layer(UnattendedEvaluationInput {
        config: &explore_config,
        paths: input.paths,
        run_id: &new_layer_run_id("screen"),
        patch: &patch,
        codex: Some(&codex_result),
        metadata: unattended_metadata(
            input.config,
            input.state,
            "screen",
            input.category,
            MetadataLinks {
                parent_run_id: Some(&parent_run_id),
                macro_trigger: input.macro_trigger,
                ..MetadataLinks::default()
            },
        ),
        commit: false,
        base_ref: &base_ref,
    })?;
    if !score_accepted(&screen_record) {
        update_unattended_rejection_counters(input.state, input.category);
        git_ops::reject_candidate(&input.config.workspace, &patch, true)?;
        return Ok(LayeredCycleOutcome::Rejected);
    }
    let validate_config = unattended_config(input.config, "fast", input.category, 1);
    let validate_run_id = new_layer_run_id(if input.kind.is_macro() {
        "macro-validate"
    } else {
        "validate"
    });
    let validate_record = evaluate_unattended_layer(UnattendedEvaluationInput {
        config: &validate_config,
        paths: input.paths,
        run_id: &validate_run_id,
        patch: &patch,
        codex: Some(&codex_result),
        metadata: unattended_metadata(
            input.config,
            input.state,
            if input.kind.is_macro() {
                "macro_validate"
            } else {
                "validate"
            },
            input.category,
            MetadataLinks {
                parent_run_id: Some(&parent_run_id),
                promoted_from_run_id: screen_record
                    .get("run_id")
                    .and_then(serde_json::Value::as_str),
                macro_trigger: input.macro_trigger,
                ..MetadataLinks::default()
            },
        ),
        commit: true,
        base_ref: &base_ref,
    })?;
    if validate_record["accepted"].as_bool().unwrap_or(false) {
        input.state.accepted_count += 1;
        input.state.consecutive_promotion_failures = 0;
        if input.category == EvaluationCategory::Competitive {
            input.state.competitive_promotion_failures = 0;
        }
        return Ok(LayeredCycleOutcome::Accepted);
    }
    update_unattended_rejection_counters(input.state, input.category);
    git_ops::reject_candidate(&input.config.workspace, &patch, true)?;
    Ok(LayeredCycleOutcome::Rejected)
}

struct UnattendedEvaluationInput<'a> {
    config: &'a Config,
    paths: &'a history::HistoryPaths,
    run_id: &'a str,
    patch: &'a git_ops::PatchSnapshot,
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
    patch: &'a git_ops::PatchSnapshot,
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
    let score = scoring::score_evaluation(&evaluation.observation, previous_run.as_ref());
    let commit = if input.commit && score.accepted {
        write_adopted_optimization_document(
            &input.config.workspace,
            input.run_id,
            input.patch,
            &score,
            &evaluation,
        )?;
        Some(git_ops::commit_candidate(
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
    patch: &git_ops::PatchSnapshot,
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
    patch: &git_ops::PatchSnapshot,
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
    let score = scoring::score_evaluation(&input.evaluation.observation, previous.as_ref());
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

fn unattended_config(
    config: &Config,
    profile: &str,
    category: EvaluationCategory,
    codex_timeout_seconds: u64,
) -> Config {
    let mut next = config.clone();
    next.profile = profile.to_owned();
    next.categories = Some(CategorySet::single(category));
    next.codex_timeout_seconds = codex_timeout_seconds.max(1);
    next.use_current_candidate = false;
    next
}

fn unattended_metadata(
    config: &Config,
    state: &UnattendedState,
    layer: &str,
    category: EvaluationCategory,
    links: MetadataLinks<'_>,
) -> serde_json::Value {
    serde_json::json!({
        "strategy": config.strategy.label(),
        "layer": layer,
        "parent_run_id": links.parent_run_id,
        "promoted_from_run_id": links.promoted_from_run_id,
        "macro_trigger": links.macro_trigger,
        "category_focus": category.label(),
        "promotion_decision": links.promotion_decision,
        "wall_clock_started_at": state.started_at.to_string(),
        "wall_clock_elapsed_seconds": state.elapsed_seconds(unix_timestamp()),
    })
}

fn update_unattended_rejection_counters(state: &mut UnattendedState, category: EvaluationCategory) {
    state.consecutive_promotion_failures += 1;
    if category == EvaluationCategory::Competitive {
        state.competitive_promotion_failures += 1;
    }
}

fn next_unattended_category(state: &mut UnattendedState) -> EvaluationCategory {
    let category = CATEGORY_ROTATION[state.category_index % CATEGORY_ROTATION.len()];
    state.category_index += 1;
    category
}

fn selected_or_default_category(config: &Config) -> EvaluationCategory {
    let Some(categories) = &config.categories else {
        return EvaluationCategory::Competitive;
    };
    for category in CATEGORY_ROTATION {
        if categories.contains(category) {
            return category;
        }
    }
    EvaluationCategory::Competitive
}

fn macro_trigger(
    config: &Config,
    paths: &history::HistoryPaths,
    state: &UnattendedState,
) -> Result<Option<String>, String> {
    if state.competitive_promotion_failures >= config.macro_after_competitive_failures {
        return Ok(Some(
            "competitive promotion failures reached threshold".to_owned(),
        ));
    }
    if state.consecutive_empty_candidates >= config.macro_after_empty_candidates {
        return Ok(Some("empty candidates reached macro threshold".to_owned()));
    }
    competitive_gap_trigger(paths)
}

fn competitive_gap_trigger(paths: &history::HistoryPaths) -> Result<Option<String>, String> {
    let latest = history::previous_scored_run_for_workload(paths, "fast", Some("competitive"))?;
    let best = history::best_accepted_run_for_workload(paths, "fast", Some("competitive"))?;
    let Some(latest) = latest else {
        return Ok(None);
    };
    let Some(best) = best else {
        return Ok(None);
    };
    let latest_value = number(&latest, "competitive_capability");
    let best_value = number(&best, "competitive_capability");
    if best_value - latest_value > COMPETITIVE_GAP_EPSILON {
        Ok(Some(format!(
            "competitive capability gap {:.6} exceeds {:.6}",
            best_value - latest_value,
            COMPETITIVE_GAP_EPSILON
        )))
    } else {
        Ok(None)
    }
}

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
    let patch = git_ops::capture_patch(&config.workspace, paths, &run_id, "HEAD")?;
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

fn unattended_sleep_seconds(config: &Config, outcome: LayeredCycleOutcome) -> u64 {
    match outcome {
        LayeredCycleOutcome::Accepted => config.cooldown_after_accept_seconds,
        LayeredCycleOutcome::CodexTimeout => config.cooldown_after_timeout_seconds,
        LayeredCycleOutcome::Rejected
        | LayeredCycleOutcome::EmptyCandidate
        | LayeredCycleOutcome::CodexFailed => config.cycle_sleep_seconds,
    }
}

fn score_accepted(record: &serde_json::Value) -> bool {
    record
        .get("score_accepted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn stop_reason_uses_default_unattended_accept_limit() {
        let config = Config::parse(vec![
            "loop".to_owned(),
            "--strategy".to_owned(),
            "unattended-layered".to_owned(),
        ])
        .expect("config should parse");
        let mut state = UnattendedState::new(100);
        state.accepted_count = UNATTENDED_ACCEPT_LIMIT;

        let reason = unattended_stop_reason(&config, &state, 120);

        assert_eq!(reason.as_deref(), Some("accepted limit reached"));
    }

    #[test]
    fn category_rotation_starts_with_competitive() {
        let mut state = UnattendedState::new(100);

        assert_eq!(
            next_unattended_category(&mut state),
            EvaluationCategory::Competitive
        );
        assert_eq!(
            next_unattended_category(&mut state),
            EvaluationCategory::SemanticVector
        );
    }

    #[test]
    fn macro_trigger_uses_competitive_failure_threshold() {
        let config = Config::parse(vec![
            "loop".to_owned(),
            "--strategy".to_owned(),
            "unattended-layered".to_owned(),
            "--macro-after-competitive-failures".to_owned(),
            "2".to_owned(),
        ])
        .expect("config should parse");
        let mut state = UnattendedState::new(100);
        state.competitive_promotion_failures = 2;
        let paths = history::HistoryPaths::new(&temp_workspace("macro-trigger"));

        let reason = macro_trigger(&config, &paths, &state).expect("macro trigger");

        assert_eq!(
            reason.as_deref(),
            Some("competitive promotion failures reached threshold")
        );
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_owned());
        std::env::temp_dir().join(format!("relay-knowledge-{name}-{suffix}"))
    }
}
