use std::path::Path;

use crate::{
    config::CategorySet,
    history::{
        HistoryPaths, best_accepted_run_for_profile, best_accepted_run_for_workload,
        memory::{
            historical_patch_memory_index, progressive_memory_index,
            rejection_recovery_memory_review,
        },
        synthesis::synthesize_history,
    },
};

use super::history_context::{recent_rejections, run_brief};

pub fn build_prompt(
    paths: &HistoryPaths,
    workspace: &Path,
    run_id: &str,
    profile: &str,
    categories: Option<&CategorySet>,
) -> String {
    let category_focus_key = categories.map(CategorySet::focus_key);
    let best = best_accepted_run_for_workload(paths, profile, category_focus_key.as_deref())
        .ok()
        .flatten();
    let profile_best = best_accepted_run_for_profile(paths, profile).ok().flatten();
    let best_summary = best
        .as_ref()
        .map(run_brief)
        .unwrap_or_else(|| "none for this profile/category".to_owned());
    let profile_best_summary = profile_best
        .as_ref()
        .map(run_brief)
        .unwrap_or_else(|| "none for this profile".to_owned());
    let rejected = recent_rejections(paths);
    let recovery_memory = rejection_recovery_memory_review(paths, 5);
    let progressive_memory = progressive_memory_index(paths, 12);
    let patch_memory = historical_patch_memory_index(paths, 12);
    let history_synthesis = synthesize_history(paths, profile);
    let category_focus = categories
        .map(|items| items.labels().join(", "))
        .unwrap_or_else(|| "profile default workload".to_owned());
    format!(
        r#"You are running inside relay-knowledge self-iteration run {run_id}.

Goal:
- Preserve foundational capability, competitive capability, semantic/vector retrieval, and stability as protected floors.
- Improve multi-repository code retrieval, indexing throughput, semantic/vector retrieval, research alignment, and measured performance.
- Treat tools/self_iteration/cases.json as the target workload. Improve general parser, graph, retrieval, indexing, ranking, and service behavior instead of enumerating fixture strings.
- Any implementation candidate must update docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md with algorithm, architecture, invariants, expected impact, and risks. Evaluation-set-only candidates may instead update the matching benchmark specification document, such as docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md or docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md.

Constraints:
- Follow AGENTS.md and hard architecture constraints.
- Keep this self-iteration harness independent under tools/self_iteration.
- Do not create commits yourself; the harness owns accepted commits.
- Code graph import hits whose external dependency target remains unresolved
  may use the product's internal grep fallback over the current indexed
  repository source. Treat `text_fallback` results and the external dependency diagnostic
  as source-text evidence, not as proof that the dependency library itself is
  indexed in the code graph.
- For your own codebase inspection, prefer `rg`. If this machine does not
  have `rg`, use bounded `grep -RIn` searches with excluded build and VCS
  directories instead of stopping exploration or weakening the search.

Workspace: {workspace}
Evaluation profile: {profile}
Evaluation category focus: {category_focus}
Historical context:
- Best accepted for this profile/category: {best_summary}
- Best accepted for this profile: {profile_best_summary}
Historical synthesis:
{history_synthesis}

Recent rejected v2 attempts:
{rejected}

Rejected recovery memory:
{recovery_memory}

Progressive memory index:
{progressive_memory}

Historical patch memory index:
{patch_memory}

Make one concrete candidate code change now. Before editing, use the historical synthesis to decide whether this should be a broader algorithmic change rather than another small local tweak. In your final notes, state which accepted strategy or rejected pattern the candidate builds on or avoids.
"#,
        workspace = workspace.display(),
    )
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod prompt_tests;
