use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::history::{self, HistoryPaths};

const RECENT_REJECTION_LIMIT: usize = 12;
const RECENT_ACCEPTED_LIMIT: usize = 3;
const HOTSPOT_LIMIT: usize = 5;
const LOCAL_IMPROVEMENT_LIMIT: usize = 3;
const SYNTHESIS_CHAR_LIMIT: usize = 8_000;

#[derive(Debug, Default)]
struct RejectionCluster {
    count: usize,
    latest_run_id: String,
    latest_score: f64,
    changed_paths: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct Hotspot {
    count: usize,
    latest_run_id: String,
}

pub fn synthesize_history(paths: &HistoryPaths, profile: &str) -> String {
    let Ok(runs) = history::load_runs(paths) else {
        return "History synthesis unavailable because runs-v2.jsonl could not be read.".to_owned();
    };
    let mut scoped = runs
        .iter()
        .filter(|run| {
            run_profile(run) == profile
                && run.get("score").is_some()
                && !history::is_evaluate_run(run)
        })
        .collect::<Vec<_>>();
    scoped.sort_by(|left, right| {
        string_field(left, "timestamp").cmp(&string_field(right, "timestamp"))
    });
    if scoped.is_empty() {
        return format!("No scored self-iteration history recorded for profile `{profile}` yet.");
    }

    let mut lines = vec![format!(
        "History synthesis for profile `{profile}`. Treat the latest scored run as the acceptance comparison baseline; use patch/report paths only when this digest points at a matching cluster."
    )];
    if let Some(latest) = scoped.last() {
        lines.push(format!("- Latest scored baseline: {}", run_summary(latest)));
    }
    if let Some(best) = best_accepted(&scoped) {
        lines.push(format!(
            "- Best accepted run: {} | protected floors: {}",
            run_summary(best),
            protected_floors(best)
        ));
    } else {
        lines.push("- Best accepted run: none recorded for this profile.".to_owned());
    }

    lines.extend(recent_accepted_lines(&scoped));
    lines.extend(rejection_cluster_lines(&scoped));
    lines.extend(degradation_hotspot_lines(&scoped));
    lines.extend(local_improvement_lines(&scoped));
    lines.push(
        "- Planning rule: if recent small edits only improved local metrics, choose a broader algorithmic change that preserves the listed protected floors and explicitly avoids the repeated rejection cluster."
            .to_owned(),
    );
    cap_synthesis(lines.join("\n"))
}

fn recent_accepted_lines(scoped: &[&Value]) -> Vec<String> {
    let accepted = scoped
        .iter()
        .rev()
        .filter(|run| history::adopted(run))
        .take(RECENT_ACCEPTED_LIMIT)
        .map(|run| {
            format!(
                "{} score={:.6} paths={} improvements={}",
                string_field(run, "run_id"),
                number_field(run, "score"),
                compact_list(&changed_paths(run), 3),
                compact_changes(value_array(run, "improvements"), 3)
            )
        })
        .collect::<Vec<_>>();
    if accepted.is_empty() {
        return vec!["- Recent accepted strategies: none recorded.".to_owned()];
    }
    vec![format!(
        "- Recent accepted strategies to build on: {}.",
        accepted.join(" | ")
    )]
}

fn rejection_cluster_lines(scoped: &[&Value]) -> Vec<String> {
    let mut clusters = BTreeMap::<String, RejectionCluster>::new();
    for run in scoped
        .iter()
        .rev()
        .filter(|run| !history::adopted(run))
        .take(RECENT_REJECTION_LIMIT)
    {
        let reasons = string_array(run, "reject_reasons");
        let key = if reasons.is_empty() {
            "unclassified rejection".to_owned()
        } else {
            reasons.join("; ")
        };
        let entry = clusters.entry(key).or_default();
        entry.count += 1;
        if entry.latest_run_id.is_empty() {
            entry.latest_run_id = string_field(run, "run_id");
            entry.latest_score = number_field(run, "score");
        }
        entry.changed_paths.extend(changed_paths(run));
    }
    let mut clusters = clusters.into_iter().collect::<Vec<_>>();
    clusters.sort_by(|(_, left), (_, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest_run_id.cmp(&left.latest_run_id))
    });
    let lines = clusters
        .into_iter()
        .filter(|(_, cluster)| cluster.count > 1)
        .take(HOTSPOT_LIMIT)
        .map(|(reason, cluster)| {
            format!(
                "{} x{} latest={} score={:.6} paths={}",
                compact_text(&reason, 180),
                cluster.count,
                cluster.latest_run_id,
                cluster.latest_score,
                compact_set(&cluster.changed_paths, 4)
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["- Repeated rejection clusters: none in the recent window.".to_owned()]
    } else {
        vec![format!(
            "- Repeated rejection clusters: {}.",
            lines.join(" | ")
        )]
    }
}

fn degradation_hotspot_lines(scoped: &[&Value]) -> Vec<String> {
    let mut hotspots = BTreeMap::<String, Hotspot>::new();
    for run in scoped.iter().rev().take(RECENT_REJECTION_LIMIT) {
        for change in value_array(run, "degradations") {
            let Some(name) = change_name(change) else {
                continue;
            };
            let kind = change
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("change");
            let key = format!("{kind}:{name}");
            let entry = hotspots.entry(key).or_default();
            entry.count += 1;
            if entry.latest_run_id.is_empty() {
                entry.latest_run_id = string_field(run, "run_id");
            }
        }
    }
    let mut hotspots = hotspots.into_iter().collect::<Vec<_>>();
    hotspots.sort_by(|(left_name, left), (right_name, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left_name.cmp(right_name))
    });
    let lines = hotspots
        .into_iter()
        .filter(|(_, hotspot)| hotspot.count > 1)
        .take(HOTSPOT_LIMIT)
        .map(|(name, hotspot)| {
            format!("{name} x{} latest={}", hotspot.count, hotspot.latest_run_id)
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["- Repeated degradation hotspots: none in the recent window.".to_owned()]
    } else {
        vec![format!(
            "- Repeated degradation hotspots: {}.",
            lines.join(" | ")
        )]
    }
}

fn local_improvement_lines(scoped: &[&Value]) -> Vec<String> {
    let lines = scoped
        .iter()
        .rev()
        .filter(|run| {
            !history::adopted(run)
                && !value_array(run, "improvements").is_empty()
                && string_array(run, "reject_reasons")
                    .iter()
                    .any(|reason| reason.contains("did not improve score"))
        })
        .take(LOCAL_IMPROVEMENT_LIMIT)
        .map(|run| {
            format!(
                "{} score={:.6} delta={} improved={} degraded={}",
                string_field(run, "run_id"),
                number_field(run, "score"),
                score_delta(run),
                compact_changes(value_array(run, "improvements"), 3),
                compact_changes(value_array(run, "degradations"), 3)
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["- Local improvements that did not win: none recorded.".to_owned()]
    } else {
        vec![format!(
            "- Local improvements that did not win: {}.",
            lines.join(" | ")
        )]
    }
}

fn best_accepted<'a>(runs: &'a [&Value]) -> Option<&'a Value> {
    runs.iter()
        .copied()
        .filter(|run| history::adopted(run))
        .max_by(|left, right| {
            number_field(left, "score")
                .partial_cmp(&number_field(right, "score"))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn run_summary(run: &Value) -> String {
    let status = if history::adopted(run) {
        "accepted"
    } else {
        "rejected"
    };
    let reasons = string_array(run, "reject_reasons");
    let reason_text = if reasons.is_empty() {
        "no reject reasons".to_owned()
    } else {
        compact_text(&reasons.join("; "), 180)
    };
    format!(
        "{} status={} score={:.6} foundational={:.6} competitive={:.6} accuracy={:.6} performance={:.6} stability={:.6} reasons={}",
        string_field(run, "run_id"),
        status,
        number_field(run, "score"),
        number_field(run, "foundational_capability"),
        number_field(run, "competitive_capability"),
        number_field(run, "accuracy"),
        number_field(run, "performance"),
        number_field(run, "stability"),
        reason_text
    )
}

fn protected_floors(run: &Value) -> String {
    format!(
        "foundational={:.6}, competitive={:.6}, semantic_vector={:.6}, stability={:.6}",
        number_field(run, "foundational_capability"),
        number_field(run, "competitive_capability"),
        number_field(run, "semantic_vector"),
        number_field(run, "stability")
    )
}

fn score_delta(run: &Value) -> String {
    value_array(run, "degradations")
        .iter()
        .chain(value_array(run, "improvements"))
        .find(|change| {
            change.get("kind").and_then(Value::as_str) == Some("score_component")
                && change.get("name").and_then(Value::as_str) == Some("score")
        })
        .and_then(|change| {
            Some(format!(
                "{:+.6}",
                change.get("current")?.as_f64()? - change.get("previous")?.as_f64()?
            ))
        })
        .unwrap_or_else(|| "within epsilon".to_owned())
}

fn compact_changes(changes: &[Value], limit: usize) -> String {
    let lines = changes
        .iter()
        .take(limit)
        .filter_map(|change| {
            let name = change_name(change)?;
            Some(format!(
                "{}:{} {}->{}",
                change
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("change"),
                name,
                json_scalar(change.get("previous")),
                json_scalar(change.get("current"))
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "none".to_owned()
    } else {
        lines.join(", ")
    }
}

fn changed_paths(run: &Value) -> Vec<String> {
    run.get("optimization_plan")
        .and_then(|plan| plan.get("changed_paths"))
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn compact_list(values: &[String], limit: usize) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    let mut selected = values.iter().take(limit).cloned().collect::<Vec<_>>();
    if values.len() > limit {
        selected.push(format!("+{} more", values.len() - limit));
    }
    selected.join(",")
}

fn compact_set(values: &BTreeSet<String>, limit: usize) -> String {
    compact_list(&values.iter().cloned().collect::<Vec<_>>(), limit)
}

fn compact_text(value: &str, limit: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= limit {
        return compact;
    }
    compact.chars().take(limit).collect::<String>()
}

fn change_name(change: &Value) -> Option<String> {
    change
        .get("name")
        .or_else(|| change.get("case_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn value_array<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn string_array(value: &Value, name: &str) -> Vec<String> {
    value_array(value, name)
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn string_field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn number_field(value: &Value, name: &str) -> f64 {
    value.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn run_profile(run: &Value) -> &str {
    run.get("profile").and_then(Value::as_str).unwrap_or("full")
}

fn json_scalar(value: Option<&Value>) -> String {
    value
        .map(Value::to_string)
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned()
}

fn cap_synthesis(value: String) -> String {
    if value.len() <= SYNTHESIS_CHAR_LIMIT {
        return value;
    }
    let mut capped = value.chars().take(SYNTHESIS_CHAR_LIMIT).collect::<String>();
    capped.push_str(
        "\n- History synthesis truncated to the bounded prompt budget; use memory or patch paths only for matching details.",
    );
    capped
}

#[cfg(test)]
#[path = "synthesis_tests.rs"]
mod tests;
