struct JudgePromptInput<'a> {
    workspace: &'a Path,
    suite: &'a Value,
    generated_diff: bool,
    candidate_diff: &'a str,
    gates: &'a [GateObservation],
    cases: &'a [CaseObservation],
    metrics: &'a [MetricObservation],
    repo_reports: &'a [RepoReport],
}

fn build_judge_prompt(input: JudgePromptInput<'_>) -> String {
    let max_doc_chars = number_or(input.suite, "max_doc_chars", 3000) as usize;
    let max_diff_chars = number_or(input.suite, "max_diff_chars", 30000) as usize;
    let mut diff = input.candidate_diff.trim().to_owned();
    if diff.chars().count() > max_diff_chars {
        diff = diff.chars().take(max_diff_chars).collect();
        diff.push_str("\n...diff truncated...");
    }
    format!(
        "You are the relay-knowledge research judge.\nReturn only one strict JSON object with passed, confidence, overall_score, scores, summary, evidence, risks, recommended_cases, capability_delta, research_gaps.\nThe scores object must include every required dimension from the suite requirements and each score must be a number from 0.0 to 1.0. Judge the candidate for general research competitiveness, not fixture memorization.\n\nDeterministic summary:\n{}\n\nJudge suite requirements:\n{}\n\nCandidate diff:\n```diff\n{}\n```\n\nReference document excerpts:\n{}",
        deterministic_summary(
            input.gates,
            input.cases,
            input.metrics,
            input.repo_reports,
            input.generated_diff
        ),
        judge_suite_requirements(input.suite),
        diff,
        document_excerpts(input.workspace, input.suite, max_doc_chars)
    )
}

fn judge_suite_requirements(suite: &Value) -> String {
    serde_json::json!({
        "competitive_feature_targets": suite.get("competitive_feature_targets").cloned().unwrap_or(Value::Null),
        "implementation_guardrails": suite.get("implementation_guardrails").cloned().unwrap_or(Value::Null),
        "rubric_dimensions": suite.get("rubric_dimensions").cloned().unwrap_or_else(|| serde_json::json!(required_judge_dimensions(suite))),
        "required_output_fields": ["passed", "confidence", "overall_score", "scores", "summary", "evidence", "risks", "recommended_cases", "capability_delta", "research_gaps"],
        "min_score": suite.get("min_score").cloned().unwrap_or(Value::Null),
        "min_confidence": suite.get("min_confidence").cloned().unwrap_or(Value::Null),
        "min_anti_fixture_special_casing": suite.get("min_anti_fixture_special_casing").cloned().unwrap_or(Value::Null),
        "min_dimension_score": suite.get("min_dimension_score").cloned().unwrap_or(Value::Null),
    })
    .to_string()
}

fn deterministic_summary(
    gates: &[GateObservation],
    cases: &[CaseObservation],
    metrics: &[MetricObservation],
    repo_reports: &[RepoReport],
    generated_diff: bool,
) -> String {
    serde_json::json!({
        "generated_diff": generated_diff,
        "gate_count": gates.len(),
        "failed_gates": gates.iter().filter(|gate| !gate.passed).map(|gate| &gate.name).collect::<Vec<_>>(),
        "case_count": cases.len(),
        "failed_cases": cases.iter().filter(|case| !case.passed).take(16).map(|case| &case.case_id).collect::<Vec<_>>(),
        "objective_scores": objective_score_summary(cases),
        "metrics": metrics.iter().take(16).map(|metric| format!("{}={}", metric.name, metric.value)).collect::<Vec<_>>(),
        "metric_budget_failures": metrics.iter().filter(|metric| metric.key && metric.budget.is_some() && metric.score() < 1.0).map(|metric| format!("{}={} budget={}", metric.name, metric.value, metric.budget.unwrap_or_default())).collect::<Vec<_>>(),
        "report_sections": repo_reports.iter().map(|report| &report.repository).collect::<Vec<_>>(),
    })
    .to_string()
}

fn objective_score_summary(cases: &[CaseObservation]) -> Value {
    let mut grouped: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for case in cases {
        grouped
            .entry(case.objective.clone())
            .or_default()
            .push(case.score());
    }
    serde_json::json!(
        grouped
            .into_iter()
            .map(|(objective, scores)| {
                let average = if scores.is_empty() {
                    0.0
                } else {
                    scores.iter().sum::<f64>() / scores.len() as f64
                };
                (objective, average)
            })
            .collect::<BTreeMap<_, _>>()
    )
}

fn document_excerpts(workspace: &Path, suite: &Value, max_doc_chars: usize) -> String {
    let default_docs = vec![
        "docs/zh/02-capabilities/15-evaluation-and-quality-gates.md".to_owned(),
        "docs/zh/03-architecture-specs/02-engineering-hard-constraints.md".to_owned(),
        "docs/zh/04-research/08-competitive-performance-research-2026.md".to_owned(),
    ];
    let docs = if array_field(suite, "documents").is_empty() {
        default_docs
    } else {
        string_vec(suite, "documents")
    };
    docs.into_iter()
        .map(|relative| {
            let text = fs::read_to_string(workspace.join(&relative))
                .unwrap_or_else(|_| "(missing)".to_owned());
            let excerpt = text.chars().take(max_doc_chars).collect::<String>();
            format!("## {relative}\n{excerpt}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
