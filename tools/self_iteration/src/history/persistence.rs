pub fn write_report(paths: &HistoryPaths, run_id: &str, report: &Value) -> Result<PathBuf, String> {
    paths.ensure()?;
    let path = paths.reports.join(format!("{run_id}.json"));
    fs::write(
        &path,
        serde_json::to_string_pretty(report).map_err(|error| error.to_string())? + "\n",
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(path)
}

pub fn append_run(paths: &HistoryPaths, record: &Value) -> Result<(), String> {
    paths.ensure()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.runs_jsonl)
        .map_err(|error| format!("failed to open {}: {error}", paths.runs_jsonl.display()))?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(record).map_err(|error| error.to_string())?
    )
    .map_err(|error| format!("failed to append {}: {error}", paths.runs_jsonl.display()))
}

pub struct RunRecordInput<'a> {
    pub run_id: &'a str,
    pub timestamp: &'a str,
    pub profile: &'a str,
    pub category_focus: Option<&'a str>,
    pub selected_categories: &'a [&'a str],
    pub report_path: &'a Path,
    pub commit: Option<&'a str>,
    pub score: &'a ScoreBreakdown,
    pub observation: &'a EvaluationObservation,
}

pub fn make_run_record(input: RunRecordInput<'_>) -> Value {
    let committed = input.commit.is_some();
    let selected_categories = if input.selected_categories.is_empty() {
        Value::Null
    } else {
        serde_json::json!(input.selected_categories)
    };
    serde_json::json!({
        "run_id": input.run_id,
        "timestamp": input.timestamp,
        "profile": input.profile,
        "category_focus": input.category_focus,
        "selected_categories": selected_categories,
        "accepted": committed,
        "score_accepted": input.score.accepted,
        "committed": committed,
        "adoption_status": adoption_status(committed, input.score.accepted),
        "score": rounded(input.score.score),
        "foundational_capability": rounded(input.score.foundational_capability),
        "competitive_capability": rounded(input.score.competitive_capability),
        "accuracy": rounded(input.score.accuracy),
        "semantic_vector": rounded(input.score.semantic_vector),
        "research_judge": input.score.research_judge.map(rounded),
        "performance": rounded(input.score.performance),
        "stability": rounded(input.score.stability),
        "base_score": rounded(input.score.base_score),
        "capability_ceiling_bonus": rounded(input.score.capability_ceiling_bonus),
        "scoring_policy": input.score.scoring_policy.as_str(),
        "reject_reasons": input.score.reject_reasons,
        "degradations": input.score.degradations,
        "improvements": input.score.improvements,
        "metric_budget_failures": input.score.metric_budget_failures,
        "generated_diff": input.observation.generated_diff,
        "report": input.report_path.display().to_string(),
        "commit": input.commit,
        "gates": input.observation.gates,
        "cases": input.observation.cases,
        "metrics": input.observation.metrics,
    })
}
