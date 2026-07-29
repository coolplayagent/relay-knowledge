fn selected_categories_value(selected_categories: &[&str]) -> serde_json::Value {
    if selected_categories.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(selected_categories)
    }
}

fn patch_metadata(patch: &PatchSnapshot) -> serde_json::Value {
    serde_json::json!({
        "path": patch.path.display().to_string(),
        "sha256": patch.sha256,
        "bytes": patch.diff.len(),
        "has_diff": patch.has_diff(),
        "base_ref": patch.base_ref,
    })
}

fn optimization_plan(
    patch: &PatchSnapshot,
    score: &scoring::ScoreBreakdown,
    codex: Option<&codex::CodexResult>,
) -> serde_json::Value {
    let codex_notes = codex.map(|result| {
        history::memory::compact_prompt_text(&format!("{}\n{}", result.stdout, result.stderr), 1200)
    });
    serde_json::json!({
        "changed_paths": git_ops::changed_paths_from_diff(&patch.diff),
        "key_improvements": history::memory::compact_score_changes(&score.improvements, 8),
        "known_degradations": history::memory::compact_score_changes(&score.degradations, 8),
        "reject_reasons": score.reject_reasons,
        "codex_notes": codex_notes,
    })
}

fn comparison_baseline(
    paths: &history::HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
    previous_run: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let best_accepted = history::best_accepted_run_for_workload(paths, profile, category_focus)?;
    let profile_best_accepted = history::best_accepted_run_for_profile(paths, profile)?;
    Ok(serde_json::json!({
        "comparison_kind": "latest_scored_workload_run",
        "profile": profile,
        "category_focus": category_focus,
        "latest_run_id": previous_run.and_then(|run| run.get("run_id")).and_then(serde_json::Value::as_str),
        "latest_score": previous_run.and_then(|run| run.get("score")).and_then(serde_json::Value::as_f64),
        "latest_accepted": previous_run.map(history::adopted),
        "best_accepted_run_id": best_accepted.as_ref().and_then(|run| run.get("run_id")).and_then(serde_json::Value::as_str),
        "best_accepted_score": best_accepted.as_ref().and_then(|run| run.get("score")).and_then(serde_json::Value::as_f64),
        "profile_best_accepted_run_id": profile_best_accepted.as_ref().and_then(|run| run.get("run_id")).and_then(serde_json::Value::as_str),
        "profile_best_accepted_score": profile_best_accepted.as_ref().and_then(|run| run.get("score")).and_then(serde_json::Value::as_f64),
    }))
}
