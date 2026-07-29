use std::{fs, path::Path};

use serde_json::Value;

use crate::{
    cases::{array_field, number_or, object_field, string_field, string_or},
    command::CommandSpec,
    scoring::MetricObservation,
};

use super::super::{
    EvalRuntime, FileReport, budget, parallel_map, push_latency_metrics, run_limited,
};
use super::file_fixture::{
    create_file_fixture, evaluate_background_file_case, file_fixture_env, file_query_command,
    score_file_case,
};

pub(in crate::evaluator) fn evaluate_file_fixtures(
    runtime: &EvalRuntime,
    run_home: &Path,
    cases_config: &Value,
) -> Result<FileReport, String> {
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut metrics = Vec::new();
    let fixture_root = run_home.join("file-fixtures");
    fs::create_dir_all(&fixture_root)
        .map_err(|error| format!("failed to create {}: {error}", fixture_root.display()))?;
    let fixtures: Vec<(String, Value)> = object_field(cases_config, "file_fixtures")
        .map(|object| {
            object
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    let all_cases = array_field(cases_config, "file_query_cases");
    eprintln!(
        "[self-iterate] file fixtures prepared fixtures={} query_cases={}",
        fixtures.len(),
        all_cases.len()
    );
    for (fixture_name, fixture) in fixtures {
        let fixture_cases = all_cases
            .iter()
            .filter(|case| {
                string_field(case, "fixture") == Some(fixture_name.as_str())
                    && string_field(case, "mode") != Some("background_auto_index")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !fixture_cases.is_empty() {
            let root = fixture_root.join(&fixture_name);
            create_file_fixture(&root, &fixture)?;
            let fixture_env = file_fixture_env(&runtime.env, &root);
            let index = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{fixture_name}_files_index"),
                    vec![
                        runtime.binary.display().to_string(),
                        "files".to_owned(),
                        "index".to_owned(),
                        "--root".to_owned(),
                        root.display().to_string(),
                        "--source".to_owned(),
                        "local-files".to_owned(),
                        "--format".to_owned(),
                        "json".to_owned(),
                    ],
                    &runtime.workspace,
                    Some(fixture_env.clone()),
                    runtime.timeout,
                ),
            );
            metrics.push(MetricObservation {
                name: format!("{fixture_name}_file_index_ms"),
                value: index.duration_ms as f64,
                budget: budget(&fixture, "index_budget_ms"),
                lower_is_better: true,
                key: true,
            });
            let index_passed = index.passed();
            commands.push(index);
            if index_passed {
                let results = parallel_map(fixture_cases, runtime.query_jobs.max(1), {
                    let runtime = runtime.clone();
                    let fixture_env = fixture_env.clone();
                    let fixture_name = fixture_name.clone();
                    move |case| {
                        let query = run_limited(
                            &runtime.limiter,
                            CommandSpec::new(
                                format!("{}_{}", fixture_name, string_or(&case, "id", "case")),
                                file_query_command(&runtime.binary, &case),
                                &runtime.workspace,
                                Some(fixture_env.clone()),
                                runtime.timeout.min(number_or(&case, "timeout_seconds", 10)),
                            ),
                        );
                        let observation = score_file_case(&fixture_name, &case, &query);
                        (query, observation)
                    }
                });
                let durations = results
                    .iter()
                    .map(|(command, _)| command.duration_ms)
                    .collect::<Vec<_>>();
                for (command, observation) in results {
                    commands.push(command);
                    cases.push(observation);
                }
                push_latency_metrics(
                    &mut metrics,
                    &fixture,
                    &format!("{fixture_name}_file_query"),
                    &durations,
                );
            }
        }
    }
    for case in all_cases
        .iter()
        .filter(|case| string_field(case, "mode") == Some("background_auto_index"))
    {
        let (command, observation, metric) =
            evaluate_background_file_case(runtime, &fixture_root, cases_config, case)?;
        commands.push(command);
        cases.push(observation);
        metrics.push(metric);
    }
    Ok(FileReport {
        commands,
        cases,
        metrics,
    })
}

#[cfg(test)]
#[path = "file_evaluation_tests.rs"]
mod tests;
