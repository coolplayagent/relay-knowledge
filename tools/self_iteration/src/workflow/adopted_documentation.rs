pub(crate) fn write_adopted_optimization_document(
    workspace: &std::path::Path,
    run_id: &str,
    patch: &PatchSnapshot,
    score: &scoring::ScoreBreakdown,
    evaluation: &evaluator::EvaluationRun,
) -> Result<(), String> {
    let path = workspace.join("docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md");
    let case_count = evaluation.observation.cases.len();
    let passed_cases = evaluation
        .observation
        .cases
        .iter()
        .filter(|case| case.passed)
        .count();
    let metrics = evaluation
        .observation
        .metrics
        .iter()
        .filter(|metric| metric.name.ends_with("_ms"))
        .take(8)
        .map(|metric| format!("{}={:.0}ms", metric.name, metric.value))
        .collect::<Vec<_>>()
        .join("; ");
    let entry = format!(
        "\n## {run_id}\n\n- patch: `{}`\n- score: {:.6} (foundational={:.6}, competitive={:.6}, accuracy={:.6}, semantic_vector={:.6}, research_judge={}, performance={:.6}, stability={:.6})\n- cases: {passed_cases}/{case_count} passed\n- changed paths: {}\n- key improvements: {}\n- known degradations: {}\n- latency metrics: {}\n\nAdopted optimization notes:\n\nRust self-iteration v2 accepted this candidate through the independent tools/self_iteration harness. The candidate is expected to improve the general retrieval, indexing, evaluation, or harness behavior described by the changed paths and recorded metrics.\n\n",
        patch.path.display(),
        score.score,
        score.foundational_capability,
        score.competitive_capability,
        score.accuracy,
        score.semantic_vector,
        score
            .research_judge
            .map(|value| format!("{value:.6}"))
            .unwrap_or_else(|| "n/a".to_owned()),
        score.performance,
        score.stability,
        git_ops::changed_paths_from_diff(&patch.diff)
            .into_iter()
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", "),
        compact_changes(&score.improvements),
        compact_changes(&score.degradations),
        if metrics.is_empty() {
            "none recorded"
        } else {
            &metrics
        },
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(entry.as_bytes())
        })
        .map_err(|error| format!("failed to append {}: {error}", path.display()))
}

fn compact_changes(changes: &[serde_json::Value]) -> String {
    let text = changes
        .iter()
        .take(8)
        .map(|item| {
            format!(
                "{}:{} {}->{}",
                item.get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("change"),
                item.get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown"),
                item.get("previous")
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                item.get("current")
                    .map(ToString::to_string)
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    if text.is_empty() {
        "none recorded".to_owned()
    } else {
        text
    }
}
