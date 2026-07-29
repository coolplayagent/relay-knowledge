use crate::{candidate_git, codex, config::EvaluationCategory};

use super::super::new_layer_run_id;
use super::{
    LayeredCycleOutcome, MetadataLinks, UnattendedAttemptInput, UnattendedEvaluationInput,
    category_rotation::update_unattended_rejection_counters,
    configuration::unattended_config,
    evaluation::{evaluate_unattended_layer, persist_empty_candidate, persist_generation_failure},
    metadata::unattended_metadata,
    outcome::score_accepted,
};

pub(super) fn run_unattended_attempt(
    input: UnattendedAttemptInput<'_>,
) -> Result<LayeredCycleOutcome, String> {
    if !input.config.use_current_candidate {
        candidate_git::ensure_clean_worktree(&input.config.workspace)?;
    }
    let parent_run_id = new_layer_run_id(input.kind.label());
    let base_ref = candidate_git::current_head(&input.config.workspace)?;
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
    let patch = candidate_git::capture_patch(
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
        candidate_git::reject_candidate(&input.config.workspace, &patch, true)?;
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
        candidate_git::reject_candidate(&input.config.workspace, &patch, true)?;
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
    candidate_git::reject_candidate(&input.config.workspace, &patch, true)?;
    Ok(LayeredCycleOutcome::Rejected)
}
