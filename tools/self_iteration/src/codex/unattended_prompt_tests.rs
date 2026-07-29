use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;

#[test]
fn unattended_prompt_explains_external_import_grep_fallback() {
    let workspace = temp_workspace("codex-unattended-prompt");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");

    let prompt = build_unattended_prompt(
        &paths,
        &workspace,
        "run-test",
        "fast",
        EvaluationCategory::Competitive,
        false,
        &json!({}),
    );

    assert!(prompt.contains("unresolved external dependencies"));
    assert!(prompt.contains("external dependency diagnostic"));
    assert!(prompt.contains("dependency library"));
    assert!(prompt.contains("text_fallback"));
    assert!(prompt.contains("If `rg` is unavailable"));
    assert!(prompt.contains("grep -RIn"));
    assert!(prompt.contains("Mutation profile: focused explore"));
    assert!(!prompt.contains("macro biological mutation"));
}

#[test]
fn macro_unattended_prompt_requires_bolder_research_mutation() {
    let workspace = temp_workspace("codex-macro-unattended-prompt");
    let paths = HistoryPaths::new(&workspace);
    paths.ensure().expect("history paths");
    fs::write(
        &paths.runs_jsonl,
        json!({
            "run_id": "accepted",
            "timestamp": "1",
            "profile": "fast",
            "category_focus": "competitive",
            "accepted": true,
            "score_accepted": true,
            "committed": true,
            "commit": "abc1234",
            "score": 0.91,
            "base_score": 0.90,
            "capability_ceiling_bonus": 0.01,
            "foundational_capability": 0.95,
            "competitive_capability": 0.82,
            "semantic_vector": 0.88,
            "research_judge": 0.77,
            "performance": 0.81,
            "stability": 1.0,
            "reject_reasons": [],
            "improvements": [],
            "degradations": []
        })
        .to_string(),
    )
    .expect("runs");

    let prompt = build_unattended_prompt(
        &paths,
        &workspace,
        "run-test",
        "fast",
        EvaluationCategory::Competitive,
        true,
        &json!({
            "research_judge_suite": {
                "competitive_feature_targets": ["P0 grouped code-query results"],
                "implementation_guardrails": ["Do not enumerate fixture strings"]
            }
        }),
    );

    assert!(prompt.contains("macro biological mutation"));
    assert!(prompt.contains("step-change in capability"));
    assert!(prompt.contains("mutation_hypothesis"));
    assert!(prompt.contains("expected_capability_jump"));
    assert!(prompt.contains("Current capability snapshot"));
    assert!(prompt.contains("research_judge=0.77"));
    assert!(prompt.contains("P0 grouped code-query results"));
}

fn temp_workspace(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    fs::create_dir_all(workspace.join(".git")).expect("workspace");
    workspace
}
