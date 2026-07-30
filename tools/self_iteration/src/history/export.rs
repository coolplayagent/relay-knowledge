use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{
    HistoryPaths,
    run_state::{adopted, adoption_status_for_run, committed, run_mode, score_accepted},
    runs::load_runs,
};

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
#[path = "export_tests.rs"]
mod export_tests;
