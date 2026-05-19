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
            runs_jsonl: root.join("runs-v2.jsonl"),
            score_csv: root.join("score-v2.csv"),
            score_svg: root.join("score-v2.svg"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<(), String> {
        for path in [&self.root, &self.reports, &self.patches, &self.work] {
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
    Ok(runs
        .into_iter()
        .filter(|run| run.get("score").is_some())
        .max_by_key(|run| {
            run.get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        }))
}

pub fn best_accepted_run(paths: &HistoryPaths) -> Result<Option<Value>, String> {
    let runs = load_runs(paths)?;
    Ok(runs
        .into_iter()
        .filter(|run| {
            run.get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .max_by(|left, right| {
            let left_score = left.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            let right_score = right.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            left_score
                .partial_cmp(&right_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }))
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

pub fn make_run_record(
    run_id: &str,
    timestamp: &str,
    report_path: &Path,
    commit: Option<&str>,
    score: &ScoreBreakdown,
    observation: &EvaluationObservation,
) -> Value {
    serde_json::json!({
        "run_id": run_id,
        "timestamp": timestamp,
        "accepted": score.accepted,
        "score": rounded(score.score),
        "foundational_capability": rounded(score.foundational_capability),
        "competitive_capability": rounded(score.competitive_capability),
        "accuracy": rounded(score.accuracy),
        "semantic_vector": rounded(score.semantic_vector),
        "research_judge": score.research_judge.map(rounded),
        "performance": rounded(score.performance),
        "stability": rounded(score.stability),
        "reject_reasons": score.reject_reasons,
        "degradations": score.degradations,
        "improvements": score.improvements,
        "metric_budget_failures": score.metric_budget_failures,
        "report": report_path.display().to_string(),
        "commit": commit,
        "gates": observation.gates,
        "cases": observation.cases,
        "metrics": observation.metrics,
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
        "run_id,timestamp,accepted,score,foundational_capability,competitive_capability,accuracy,semantic_vector,research_judge,performance,stability,commit,reject_reasons\n",
    );
    for run in runs {
        content.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv(run, "run_id"),
            csv(run, "timestamp"),
            run.get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            number(run, "score"),
            number(run, "foundational_capability"),
            number(run, "competitive_capability"),
            number(run, "accuracy"),
            number(run, "semantic_vector"),
            optional_number(run, "research_judge"),
            number(run, "performance"),
            number(run, "stability"),
            csv(run, "commit"),
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
        empty_svg(760, 280, "No self-iteration v2 scores yet")
    } else {
        score_svg(760, 280, &scored)
    };
    fs::write(path, svg).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn score_svg(width: u32, height: u32, scored: &[(&Value, f64)]) -> String {
    let pad = 36.0;
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
            let color = if run
                .get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "#16a34a"
            } else {
                "#dc2626"
            };
            format!(r#"<circle cx="{x:.1}" cy="{y:.1}" r="4" fill="{color}" />"#)
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <text x="{pad}" y="24" font-family="monospace" font-size="16" fill="#111827">relay-knowledge self-iteration v2 score</text>
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
    )
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
