pub fn build_unattended_prompt(
    paths: &HistoryPaths,
    workspace: &Path,
    run_id: &str,
    profile: &str,
    category: EvaluationCategory,
    macro_explore: bool,
    cases_config: &Value,
) -> String {
    let categories = CategorySet::single(category);
    let category_focus_key = categories.focus_key();
    let best = best_accepted_run_for_workload(paths, profile, Some(&category_focus_key))
        .ok()
        .flatten();
    let profile_best = best_accepted_run_for_profile(paths, profile).ok().flatten();
    let latest =
        crate::history::previous_scored_run_for_workload(paths, profile, Some(&category_focus_key))
            .ok()
            .flatten();
    let feature_targets = if macro_explore {
        competitive_feature_targets(cases_config, 6)
    } else {
        "Macro targets omitted for short explore; use the current category and recent rejection summary."
            .to_owned()
    };
    let guardrails = if macro_explore {
        implementation_guardrails(cases_config, 5)
    } else {
        "Do not enumerate known queries, paths, repositories, symbols, or fixture strings."
            .to_owned()
    };
    let exploration_mode = if macro_explore {
        "macro_explore"
    } else {
        "explore"
    };
    let expected_change = if macro_explore {
        "Make a larger, general competitive-capability improvement in ranking, indexing, relationship extraction, query planning, context construction, or retrieval evidence. Prefer a coherent algorithmic change over a local tweak."
    } else {
        "Make one narrow, concrete candidate improvement for the current category."
    };
    let mutation_guidance = if macro_explore {
        "Mutation profile: macro biological mutation. Make a bounded but bolder architectural or algorithmic mutation that can create a step-change in capability. Prefer one coherent subsystem-level improvement over scattered edits. In final notes include mutation_hypothesis, affected_subsystem, expected_capability_jump, and regression_containment."
    } else {
        "Mutation profile: focused explore. Keep the candidate narrow and directly tied to the selected category."
    };
    let capability_snapshot =
        capability_snapshot(latest.as_ref(), best.as_ref(), profile_best.as_ref());
    format!(
        r#"You are running relay-knowledge unattended self-iteration run {run_id}.

Mode: {exploration_mode}
Workspace: {workspace}
Screen profile: {profile}
Category focus: {category_focus}

Goal:
- {expected_change}
- Preserve foundational capability, semantic/vector retrieval, stability, and existing competitive behavior.
- Update docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md when code, tests, benchmark behavior, or harness policy changes.
- Do not create commits; the harness owns accepted commits.
- When code graph import targets are unresolved external dependencies,
  relay-knowledge may use internal grep over the current indexed repository
  source and report `text_fallback` plus an external dependency diagnostic. Use
  that as local source-text evidence only; do not infer that the external
  dependency library has been indexed.
- For your own codebase inspection, prefer `rg`. If `rg` is unavailable on
  this machine, use bounded `grep -RIn` searches with excluded build and VCS
  directories so source search still completes.

{mutation_guidance}

Baseline:
- Latest scored run: {latest_summary}
- Best accepted run for this profile/category: {best_summary}
- Best accepted run for this profile: {profile_best_summary}

Current capability snapshot:
{capability_snapshot}

Recent rejected attempts:
{rejected}

Relevant memory index:
{memory}

Competitive feature targets:
{feature_targets}

Implementation guardrails:
{guardrails}

Before editing, inspect only the files needed for this category. In your final notes, state the strategy used and why it should improve the category without fixture specialization.
"#,
        workspace = workspace.display(),
        category_focus = category.label(),
        latest_summary = latest
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile/category".to_owned()),
        best_summary = best
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile/category".to_owned()),
        profile_best_summary = profile_best
            .as_ref()
            .map(run_brief)
            .unwrap_or_else(|| "none for this profile".to_owned()),
        mutation_guidance = mutation_guidance,
        capability_snapshot = capability_snapshot,
        rejected = recent_rejections(paths),
        memory = progressive_memory_index(paths, if macro_explore { 5 } else { 3 }),
    )
}
