use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::scoring::{EvaluationObservation, ScoreBreakdown};

#[derive(Debug, Clone)]
pub struct HistoryPaths {
    pub root: PathBuf,
    pub reports: PathBuf,
    pub patches: PathBuf,
    pub work: PathBuf,
    pub memory: PathBuf,
    pub memory_index: PathBuf,
    pub memory_summaries: PathBuf,
    pub memory_details: PathBuf,
    pub memory_artifacts: PathBuf,
    pub unattended_state: PathBuf,
    pub runs_jsonl: PathBuf,
    pub score_csv: PathBuf,
    pub score_svg: PathBuf,
}

impl HistoryPaths {
    pub fn new(workspace: &Path) -> Self {
        let root = workspace
            .join(".git")
            .join("relay-knowledge-self-iteration");
        Self {
            reports: root.join("reports-v2"),
            patches: root.join("patches-v2"),
            work: root.join("work-v2"),
            memory: root.join("memory"),
            memory_index: root.join("memory").join("index.jsonl"),
            memory_summaries: root.join("memory").join("summaries"),
            memory_details: root.join("memory").join("details"),
            memory_artifacts: root.join("memory").join("artifacts"),
            unattended_state: root.join("unattended-state-v2.json"),
            runs_jsonl: root.join("runs-v2.jsonl"),
            score_csv: root.join("score-v2.csv"),
            score_svg: root.join("score-v2.svg"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<(), String> {
        for path in [
            &self.root,
            &self.reports,
            &self.patches,
            &self.work,
            &self.memory,
            &self.memory_summaries,
            &self.memory_details,
            &self.memory_artifacts,
        ] {
            fs::create_dir_all(path)
                .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        }
        Ok(())
    }
}

pub fn load_runs(paths: &HistoryPaths) -> Result<Vec<Value>, String> {
    if !paths.runs_jsonl.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&paths.runs_jsonl)
        .map_err(|error| format!("failed to read {}: {error}", paths.runs_jsonl.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("invalid run record: {error}"))
        })
        .collect()
}

pub fn previous_scored_run(paths: &HistoryPaths) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(latest_scored_run(
        runs.into_iter().filter(|run| !is_evaluate_run(run)),
    ))
}

pub fn previous_scored_run_for_workload(
    paths: &HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(latest_scored_run(runs.into_iter().filter(|run| {
        run_profile(run) == profile
            && run_category_focus(run) == category_focus
            && !is_evaluate_run(run)
    })))
}

fn latest_scored_run<I>(runs: I) -> Option<Value>
where
    I: Iterator<Item = Value>,
{
    runs.filter(|run| run.get("score").is_some())
        .max_by_key(|run| {
            run.get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        })
}

fn run_profile(run: &Value) -> &str {
    run.get("profile").and_then(Value::as_str).unwrap_or("full")
}

fn run_category_focus(run: &Value) -> Option<&str> {
    run.get("category_focus").and_then(Value::as_str)
}

pub fn best_accepted_run_for_workload(
    paths: &HistoryPaths,
    profile: &str,
    category_focus: Option<&str>,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(best_accepted_run(runs.into_iter().filter(|run| {
        run_profile(run) == profile && run_category_focus(run) == category_focus
    })))
}

pub fn best_accepted_run_for_profile(
    paths: &HistoryPaths,
    profile: &str,
) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(best_accepted_run(
        runs.into_iter().filter(|run| run_profile(run) == profile),
    ))
}

fn best_accepted_run<I>(runs: I) -> Option<Value>
where
    I: Iterator<Item = Value>,
{
    runs.filter(adopted).max_by(|left, right| {
        let left_score = left.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        let right_score = right.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        left_score
            .partial_cmp(&right_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

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
        "report": input.report_path.display().to_string(),
        "commit": input.commit,
        "gates": input.observation.gates,
        "cases": input.observation.cases,
        "metrics": input.observation.metrics,
    })
}

pub fn export_history(paths: &HistoryPaths) -> Result<(PathBuf, PathBuf), String> {
    let runs = load_runs(paths)?;
    paths.ensure()?;
    write_csv(&paths.score_csv, &runs)?;
    write_svg(&paths.score_svg, &runs)?;
    Ok((paths.score_csv.clone(), paths.score_svg.clone()))
}

fn write_csv(path: &Path, runs: &[Value]) -> Result<(), String> {
    let mut content = String::from(
        "run_id,timestamp,profile,mode,accepted,score_accepted,committed,adoption_status,score,foundational_capability,competitive_capability,accuracy,semantic_vector,research_judge,performance,stability,commit,patch_path,patch_sha256,patch_bytes,report,reject_reasons\n",
    );
    for run in runs {
        content.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv(run, "run_id"),
            csv(run, "timestamp"),
            csv(run, "profile"),
            escape_csv(&run_mode(run)),
            adopted(run),
            score_accepted(run),
            committed(run),
            escape_csv(&adoption_status_for_run(run)),
            number(run, "score"),
            number(run, "foundational_capability"),
            number(run, "competitive_capability"),
            number(run, "accuracy"),
            number(run, "semantic_vector"),
            optional_number(run, "research_judge"),
            number(run, "performance"),
            number(run, "stability"),
            csv(run, "commit"),
            escape_csv(&patch_string(run, "path")),
            escape_csv(&patch_string(run, "sha256")),
            patch_number(run, "bytes"),
            csv(run, "report"),
            escape_csv(&reject_reasons(run))
        ));
    }
    fs::write(path, content).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn write_svg(path: &Path, runs: &[Value]) -> Result<(), String> {
    let scored = runs
        .iter()
        .filter_map(|run| Some((run, run.get("score")?.as_f64()?)))
        .collect::<Vec<_>>();
    let svg = if scored.is_empty() {
        empty_svg(820, 320, "No self-iteration v2 scores yet")
    } else {
        score_svg(820, 320, &scored)
    };
    fs::write(path, svg).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn score_svg(width: u32, height: u32, scored: &[(&Value, f64)]) -> String {
    let pad = 48.0;
    let min = scored
        .iter()
        .map(|(_, score)| *score)
        .fold(f64::INFINITY, f64::min);
    let max = scored
        .iter()
        .map(|(_, score)| *score)
        .fold(f64::NEG_INFINITY, f64::max);
    let points = scaled_points(width, height, pad, min, max, scored);
    let polyline = points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    let circles = scored
        .iter()
        .zip(points.iter())
        .map(|((run, _), (x, y))| {
            let style = chart_style(run);
            let title = xml_escape(&format!(
                "{} score={:.6} {}",
                run.get("run_id").and_then(Value::as_str).unwrap_or(""),
                run.get("score").and_then(Value::as_f64).unwrap_or(0.0),
                style.label
            ));
            format!(
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="{radius:.1}" fill="{color}" stroke="{stroke}" stroke-width="{stroke_width}"><title>{title}</title></circle>"#,
                radius = style.radius,
                color = style.color,
                stroke = style.stroke,
                stroke_width = style.stroke_width
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <text x="{pad}" y="24" font-family="monospace" font-size="16" fill="#111827">relay-knowledge self-iteration v2 score</text>
  <circle cx="{legend_x}" cy="42" r="5" fill="#16a34a" stroke="#14532d" stroke-width="1.5"/><text x="{legend_text_x}" y="46" font-family="monospace" font-size="11" fill="#374151">accepted commit</text>
  <circle cx="{legend2_x}" cy="42" r="4" fill="#f59e0b" stroke="#92400e" stroke-width="1"/><text x="{legend2_text_x}" y="46" font-family="monospace" font-size="11" fill="#374151">would accept evaluation</text>
  <circle cx="{legend3_x}" cy="42" r="3.5" fill="#dc2626" stroke="#7f1d1d" stroke-width="1"/><text x="{legend3_text_x}" y="46" font-family="monospace" font-size="11" fill="#374151">rejected</text>
  <line x1="{pad}" y1="{bottom}" x2="{right}" y2="{bottom}" stroke="#d1d5db"/>
  <line x1="{pad}" y1="{pad}" x2="{pad}" y2="{bottom}" stroke="#d1d5db"/>
  <text x="4" y="{top_label}" font-family="monospace" font-size="11" fill="#6b7280">{max:.3}</text>
  <text x="4" y="{bottom_label}" font-family="monospace" font-size="11" fill="#6b7280">{min:.3}</text>
  <polyline points="{polyline}" fill="none" stroke="#2563eb" stroke-width="2"/>
  {circles}
</svg>
"##,
        bottom = height as f64 - pad,
        right = width as f64 - pad,
        top_label = pad + 4.0,
        bottom_label = height as f64 - pad + 4.0,
        legend_x = pad,
        legend_text_x = pad + 10.0,
        legend2_x = pad + 160.0,
        legend2_text_x = pad + 170.0,
        legend3_x = pad + 380.0,
        legend3_text_x = pad + 390.0,
    )
}

struct ChartStyle {
    color: &'static str,
    stroke: &'static str,
    stroke_width: &'static str,
    radius: f64,
    label: &'static str,
}

fn chart_style(run: &Value) -> ChartStyle {
    if adopted(run) {
        return ChartStyle {
            color: "#16a34a",
            stroke: "#14532d",
            stroke_width: "1.5",
            radius: 5.0,
            label: "accepted commit",
        };
    }
    if score_accepted(run) {
        return ChartStyle {
            color: "#f59e0b",
            stroke: "#92400e",
            stroke_width: "1",
            radius: 4.0,
            label: "would accept evaluation",
        };
    }
    ChartStyle {
        color: "#dc2626",
        stroke: "#7f1d1d",
        stroke_width: "1",
        radius: 3.5,
        label: "rejected",
    }
}

fn scaled_points(
    width: u32,
    height: u32,
    pad: f64,
    min: f64,
    max: f64,
    scored: &[(&Value, f64)],
) -> Vec<(f64, f64)> {
    let x_span = width as f64 - (pad * 2.0);
    let y_span = height as f64 - (pad * 2.0);
    scored
        .iter()
        .enumerate()
        .map(|(index, (_, score))| {
            let x = if scored.len() == 1 {
                width as f64 / 2.0
            } else {
                pad + x_span * index as f64 / (scored.len() - 1) as f64
            };
            let y = if (max - min).abs() < f64::EPSILON {
                height as f64 / 2.0
            } else {
                height as f64 - pad - ((*score - min) / (max - min) * y_span)
            };
            (x, y)
        })
        .collect()
}

fn empty_svg(width: u32, height: u32, message: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <text x="24" y="42" font-family="monospace" font-size="16" fill="#111827">{message}</text>
</svg>
"##
    )
}

fn reject_reasons(run: &Value) -> String {
    run.get("reject_reasons")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn committed(run: &Value) -> bool {
    run.get("committed")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            run.get("commit")
                .and_then(Value::as_str)
                .is_some_and(|commit| !commit.trim().is_empty())
        })
}

pub fn adopted(run: &Value) -> bool {
    committed(run)
}

fn score_accepted(run: &Value) -> bool {
    run.get("score_accepted")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            run.get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
}

fn run_mode(run: &Value) -> String {
    if is_evaluate_run(run) {
        "evaluate".to_owned()
    } else {
        "loop".to_owned()
    }
}

pub fn is_evaluate_run(run: &Value) -> bool {
    run.get("run_id")
        .and_then(Value::as_str)
        .is_some_and(|run_id| run_id.starts_with("manual-evaluate"))
}

fn adoption_status(committed: bool, score_accepted: bool) -> &'static str {
    if committed {
        "committed"
    } else if score_accepted {
        "would_accept"
    } else {
        "rejected"
    }
}

fn adoption_status_for_run(run: &Value) -> String {
    run.get("adoption_status")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| adoption_status(committed(run), score_accepted(run)).to_owned())
}

fn patch_string(run: &Value, name: &str) -> String {
    run.get("patch")
        .and_then(|patch| patch.get(name))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn patch_number(run: &Value, name: &str) -> u64 {
    run.get("patch")
        .and_then(|patch| patch.get(name))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn rounded(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn csv(run: &Value, name: &str) -> String {
    escape_csv(run.get(name).and_then(Value::as_str).unwrap_or(""))
}

fn number(run: &Value, name: &str) -> f64 {
    run.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn optional_number(run: &Value, name: &str) -> String {
    run.get(name)
        .and_then(Value::as_f64)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::json;

    use super::*;

    #[test]
    fn export_history_separates_score_acceptance_from_adoption() {
        let workspace = temp_workspace("history-export");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-1",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score_accepted": true,
                "committed": true,
                "adoption_status": "committed",
                "score": 0.8,
                "foundational_capability": 1.0,
                "competitive_capability": 0.8,
                "accuracy": 0.9,
                "semantic_vector": 0.0,
                "performance": 0.8,
                "stability": 1.0,
                "commit": "abc1234",
                "patch": {"path": "/tmp/run-1.patch", "sha256": "sha", "bytes": 42},
                "report": "/tmp/run-1.json",
                "reject_reasons": []
            }),
            json!({
                "run_id": "manual-evaluate-2",
                "timestamp": "2",
                "profile": "fast",
                "accepted": false,
                "score_accepted": true,
                "committed": false,
                "adoption_status": "would_accept",
                "score": 0.81,
                "foundational_capability": 1.0,
                "competitive_capability": 0.8,
                "accuracy": 0.9,
                "semantic_vector": 0.0,
                "performance": 0.81,
                "stability": 1.0,
                "commit": null,
                "patch": {"path": "/tmp/manual-evaluate-2.patch", "sha256": "sha2", "bytes": 43},
                "report": "/tmp/manual-evaluate-2.json",
                "reject_reasons": []
            }),
            json!({
                "run_id": "run-3",
                "timestamp": "3",
                "profile": "fast",
                "accepted": false,
                "score": 0.79,
                "foundational_capability": 1.0,
                "competitive_capability": 0.8,
                "accuracy": 0.9,
                "semantic_vector": 0.0,
                "performance": 0.79,
                "stability": 1.0,
                "commit": null,
                "patch": {"path": "/tmp/run-3.patch", "sha256": "sha3", "bytes": 44},
                "report": "/tmp/run-3.json",
                "reject_reasons": ["candidate did not improve score"]
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        export_history(&paths).expect("export");
        let csv = fs::read_to_string(&paths.score_csv).expect("csv");
        let svg = fs::read_to_string(&paths.score_svg).expect("svg");

        assert!(csv.contains("mode,accepted,score_accepted,committed,adoption_status"));
        assert!(csv.contains("run-1,1,fast,loop,true,true,true,committed"));
        assert!(csv.contains("manual-evaluate-2,2,fast,evaluate,false,true,false,would_accept"));
        assert!(csv.contains("/tmp/manual-evaluate-2.patch"));
        assert!(svg.contains("accepted commit"));
        assert!(svg.contains("would accept evaluation"));
        assert!(svg.contains("#16a34a"));
        assert!(svg.contains("#f59e0b"));
        assert!(svg.contains("#dc2626"));
    }

    #[test]
    fn automated_baseline_ignores_manual_evaluations() {
        let workspace = temp_workspace("history-baseline");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-1",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score_accepted": true,
                "committed": true,
                "score": 0.8,
                "commit": "abc1234"
            }),
            json!({
                "run_id": "manual-evaluate-2",
                "timestamp": "2",
                "profile": "fast",
                "accepted": false,
                "score_accepted": true,
                "committed": false,
                "score": 0.99
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        let previous = previous_scored_run_for_workload(&paths, "fast", None)
            .expect("history")
            .expect("previous run");

        assert_eq!(
            previous.get("run_id").and_then(Value::as_str),
            Some("run-1")
        );
    }

    #[test]
    fn workload_baseline_matches_category_focus() {
        let workspace = temp_workspace("history-workload-baseline");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-default",
                "timestamp": "1",
                "profile": "fast",
                "accepted": true,
                "score": 0.8
            }),
            json!({
                "run_id": "run-semantic",
                "timestamp": "2",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "selected_categories": ["semantic_vector"],
                "accepted": true,
                "committed": true,
                "commit": "semantic123",
                "score": 0.9
            }),
            json!({
                "run_id": "run-competitive",
                "timestamp": "3",
                "profile": "fast",
                "category_focus": "competitive",
                "selected_categories": ["competitive"],
                "accepted": true,
                "committed": true,
                "commit": "competitive123",
                "score": 0.95
            }),
            json!({
                "run_id": "manual-evaluate-semantic",
                "timestamp": "4",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "selected_categories": ["semantic_vector"],
                "accepted": false,
                "score": 0.99
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        let default_previous = previous_scored_run_for_workload(&paths, "fast", None)
            .expect("history")
            .expect("default previous run");
        let semantic_previous =
            previous_scored_run_for_workload(&paths, "fast", Some("semantic_vector"))
                .expect("history")
                .expect("semantic previous run");

        assert_eq!(
            default_previous.get("run_id").and_then(Value::as_str),
            Some("run-default")
        );
        assert_eq!(
            semantic_previous.get("run_id").and_then(Value::as_str),
            Some("run-semantic")
        );
    }

    #[test]
    fn profile_best_accepted_ignores_category_focus() {
        let workspace = temp_workspace("history-profile-best");
        let paths = HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let runs = [
            json!({
                "run_id": "run-competitive",
                "timestamp": "1",
                "profile": "fast",
                "category_focus": "competitive",
                "accepted": true,
                "committed": true,
                "commit": "competitive123",
                "score": 0.84
            }),
            json!({
                "run_id": "run-semantic",
                "timestamp": "2",
                "profile": "fast",
                "category_focus": "semantic_vector",
                "accepted": true,
                "committed": true,
                "commit": "semantic123",
                "score": 0.95
            }),
            json!({
                "run_id": "run-performance",
                "timestamp": "3",
                "profile": "fast",
                "category_focus": "performance",
                "accepted": false,
                "committed": false,
                "score": 0.99
            }),
            json!({
                "run_id": "run-full",
                "timestamp": "4",
                "profile": "full",
                "category_focus": "competitive",
                "accepted": true,
                "committed": true,
                "commit": "full123",
                "score": 0.98
            }),
        ];
        fs::write(
            &paths.runs_jsonl,
            runs.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("runs");

        let best = best_accepted_run_for_profile(&paths, "fast")
            .expect("history")
            .expect("profile best");

        assert_eq!(
            best.get("run_id").and_then(Value::as_str),
            Some("run-semantic")
        );
    }

    fn temp_workspace(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(workspace.join(".git")).expect("workspace");
        workspace
    }
}
