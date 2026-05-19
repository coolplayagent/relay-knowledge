use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use serde_json::Value;

use crate::{
    cases::{
        array_field, number_or, object_field, objects_by_repository, string_field, string_or,
        string_vec,
    },
    command::{CommandResult, CommandSpec, inherited_env, run_command},
    config::{Config, JobPlan},
    history::HistoryPaths,
    scoring::{
        CaseObservation, EvaluationObservation, GateObservation, MetricObservation,
        RankedAssessment, array_field as score_array_field, assess_ranked_hits, hit_matches_any,
    },
};

#[derive(Debug, Clone)]
pub struct EvaluationRun {
    pub observation: EvaluationObservation,
    pub report: Value,
}

#[derive(Debug, Clone)]
struct RepoReport {
    repository: String,
    scope: String,
    commands: Vec<CommandResult>,
    gates: Vec<GateObservation>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
    index_summary: Value,
}

#[derive(Debug, Clone)]
struct FileReport {
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
}

#[derive(Debug, Clone)]
struct EvalRuntime {
    binary: PathBuf,
    workspace: PathBuf,
    env: BTreeMap<String, String>,
    timeout: u64,
    limiter: Limiter,
    writer_lock: Arc<Mutex<()>>,
    query_jobs: usize,
}

#[derive(Debug, Clone)]
struct QualityGate {
    name: &'static str,
    command: Vec<String>,
    timeout_seconds: u64,
}

#[derive(Debug, Clone)]
enum QualityGateStage {
    Parallel(Vec<QualityGate>),
    Rails(Vec<Vec<QualityGate>>),
}

#[derive(Debug, Clone)]
struct Limiter {
    inner: Arc<(Mutex<usize>, Condvar)>,
}

struct Permit {
    inner: Arc<(Mutex<usize>, Condvar)>,
}

impl Limiter {
    fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new((Mutex::new(limit.max(1)), Condvar::new())),
        }
    }

    fn acquire(&self) -> Permit {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        while *available == 0 {
            available = condvar
                .wait(available)
                .expect("limiter lock should not be poisoned");
        }
        *available -= 1;
        Permit {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        *available += 1;
        condvar.notify_one();
    }
}

pub fn evaluate_candidate(
    config: &Config,
    paths: &HistoryPaths,
    run_id: &str,
    cases_config: &Value,
    generated_diff: bool,
    candidate_diff: &str,
) -> Result<EvaluationRun, String> {
    let job_plan = JobPlan::resolve(config);
    let limiter = Limiter::new(job_plan.global);
    let run_home = paths.work.join(run_id).join("home");
    if run_home.exists() && !config.keep_workdirs {
        fs::remove_dir_all(&run_home)
            .map_err(|error| format!("failed to remove {}: {error}", run_home.display()))?;
    }
    fs::create_dir_all(&run_home)
        .map_err(|error| format!("failed to create {}: {error}", run_home.display()))?;
    let mut commands = Vec::new();
    let mut gates = Vec::new();
    let mut cases = Vec::new();
    let mut metrics = Vec::new();
    let mut repo_reports = Vec::new();

    if !run_quality_gate_stages(
        &config.profile,
        &config.workspace,
        &limiter,
        &mut commands,
        &mut gates,
        &mut metrics,
    ) {
        return finish(FinishInput {
            config,
            generated_diff,
            gates,
            cases,
            metrics,
            commands,
            repo_reports,
            run_home,
            job_plan,
        });
    }
    if config.profile == "smoke" {
        return finish(FinishInput {
            config,
            generated_diff,
            gates,
            cases,
            metrics,
            commands,
            repo_reports,
            run_home,
            job_plan,
        });
    }

    let binary = config
        .workspace
        .join("target")
        .join("release")
        .join("relay-knowledge");
    let mut env = inherited_env();
    env.insert(
        "RELAY_KNOWLEDGE_HOME".to_owned(),
        run_home.display().to_string(),
    );
    env.entry("RUST_BACKTRACE".to_owned())
        .or_insert_with(|| "1".to_owned());
    let runtime = EvalRuntime {
        binary: binary.clone(),
        workspace: config.workspace.clone(),
        env: env.clone(),
        timeout: config.command_timeout_seconds,
        limiter: limiter.clone(),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: job_plan.queries,
    };

    let query_cases = array_field(cases_config, "query_cases");
    let grouped_cases = objects_by_repository(query_cases);
    let repository_configs = object_field(cases_config, "repositories")
        .map(|object| {
            object
                .iter()
                .map(|(name, config)| (name.clone(), config.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let repositories = repository_configs
        .iter()
        .map(|(name, config)| (name.clone(), config.clone()))
        .collect::<Vec<_>>();
    let repo_jobs = job_plan.repositories.min(job_plan.global).max(1);
    let repo_results = parallel_map(repositories, repo_jobs, {
        let grouped_cases = grouped_cases.clone();
        let profile = config.profile.clone();
        let runtime = runtime.clone();
        move |(repo_name, repo_config)| {
            if repo_config.get("profile").and_then(Value::as_str) == Some("exhaustive")
                && profile != "exhaustive"
            {
                return None;
            }
            Some(evaluate_repository(
                &runtime,
                &repo_name,
                &repo_config,
                grouped_cases.get(&repo_name).cloned().unwrap_or_default(),
            ))
        }
    });
    for report in repo_results.into_iter().flatten() {
        let report = report?;
        commands.extend(report.commands.clone());
        gates.extend(report.commands.iter().map(GateObservation::from_command));
        cases.extend(report.cases.clone());
        metrics.extend(report.metrics.clone());
        repo_reports.push(report);
    }

    for report in
        evaluate_repository_sets(&runtime, cases_config, &repository_configs, &config.profile)?
    {
        commands.extend(report.commands.clone());
        gates.extend(report.commands.iter().map(GateObservation::from_command));
        cases.extend(report.cases.clone());
        metrics.extend(report.metrics.clone());
        repo_reports.push(report);
    }

    let file_report = evaluate_file_fixtures(&runtime, &run_home, cases_config)?;
    commands.extend(file_report.commands.clone());
    gates.extend(
        file_report
            .commands
            .iter()
            .map(GateObservation::from_command),
    );
    cases.extend(file_report.cases);
    metrics.extend(file_report.metrics);

    if let Some(suite) = cases_config
        .get("semantic_vector_suite")
        .and_then(Value::as_object)
    {
        let report = evaluate_semantic_vector_suite(&runtime, &Value::Object(suite.clone()))?;
        commands.extend(report.commands.clone());
        gates.extend(report.commands.iter().map(GateObservation::from_command));
        cases.extend(report.cases.clone());
        metrics.extend(report.metrics.clone());
        repo_reports.push(report);
    }

    if let Some(suite) = cases_config
        .get("research_judge_suite")
        .and_then(Value::as_object)
    {
        let report = evaluate_research_judge_suite(JudgeEvalInput {
            workspace: &config.workspace,
            run_home: &run_home,
            env: &env,
            suite: &Value::Object(suite.clone()),
            generated_diff,
            candidate_diff,
            gates: &gates,
            cases: &cases,
            metrics: &metrics,
            repo_reports: &repo_reports,
            limiter: &limiter,
        })?;
        gates.extend(report.gates.clone());
        cases.extend(report.cases.clone());
        metrics.extend(report.metrics.clone());
        repo_reports.push(report);
    }

    finish(FinishInput {
        config,
        generated_diff,
        gates,
        cases,
        metrics,
        commands,
        repo_reports,
        run_home,
        job_plan,
    })
}

fn evaluate_repository(
    runtime: &EvalRuntime,
    repo_name: &str,
    repo_config: &Value,
    repo_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let path = PathBuf::from(string_or(repo_config, "path", ""));
    let alias = string_or(repo_config, "alias", repo_name);
    let ref_selector = string_or(repo_config, "ref", "HEAD");
    let scope = string_or(repo_config, "scope", "all").to_owned();
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut metrics = Vec::new();
    if !path.exists() {
        commands.push(CommandResult {
            name: format!("{repo_name}_repository_exists"),
            command: vec![
                "test".to_owned(),
                "-d".to_owned(),
                path.display().to_string(),
            ],
            exit_code: 1,
            duration_ms: 0,
            stdout: String::new(),
            stderr: format!("repository path is missing: {}", path.display()),
        });
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    if scope != "all" {
        commands.push(CommandResult {
            name: format!("{repo_name}_scope_is_all"),
            command: vec!["validate".to_owned(), "scope".to_owned(), scope.clone()],
            exit_code: 1,
            duration_ms: 0,
            stdout: String::new(),
            stderr: format!("self-iteration repositories must use full scope=all, got: {scope}"),
        });
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    let register = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_register"),
            register_command(&runtime.binary, &path, alias),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    commands.push(register.clone());
    if !register.passed() {
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    let index = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_index"),
            vec![
                runtime.binary.display().to_string(),
                "repo".to_owned(),
                "index".to_owned(),
                alias.to_owned(),
                "--ref".to_owned(),
                ref_selector.to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ],
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    let index_json = parse_json_output(&index.stdout);
    metrics.push(MetricObservation {
        name: format!("{repo_name}_index_ms"),
        value: index.duration_ms as f64,
        budget: budget(repo_config, "index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    metrics.push(MetricObservation {
        name: format!("{repo_name}_register_index_ms"),
        value: (register.duration_ms + index.duration_ms) as f64,
        budget: budget(repo_config, "register_index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    commands.push(index.clone());
    if !index.passed() {
        return Ok(repo_report(
            repo_name, scope, commands, cases, metrics, index_json,
        ));
    }

    let query_results = parallel_map(repo_cases, runtime.query_jobs.max(1), {
        let runtime = runtime.clone();
        let alias = alias.to_owned();
        let ref_selector = ref_selector.to_owned();
        let repo_name = repo_name.to_owned();
        move |case| {
            let query = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{}_{}", repo_name, string_or(&case, "id", "case")),
                    query_command(&runtime.binary, &alias, &ref_selector, &case),
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let observation = score_query_case(&repo_name, &case, &query);
            (query, observation)
        }
    });
    let query_durations = query_results
        .iter()
        .map(|(command, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation) in query_results {
        commands.push(command);
        cases.push(observation);
    }
    push_latency_metrics(
        &mut metrics,
        repo_config,
        &format!("{repo_name}_query"),
        &query_durations,
    );
    Ok(repo_report(
        repo_name, scope, commands, cases, metrics, index_json,
    ))
}

fn evaluate_file_fixtures(
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

fn evaluate_semantic_vector_suite(
    runtime: &EvalRuntime,
    suite: &Value,
) -> Result<RepoReport, String> {
    let scope = string_or(suite, "source_scope", "self-iteration-semantic-vector");
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut metrics = Vec::new();
    let runtime_profile = semantic_vector_runtime_profile(&runtime.env);
    if runtime_profile["external_requested"]
        .as_bool()
        .unwrap_or(false)
    {
        let env_check = semantic_vector_env_check(&runtime_profile);
        let passed = env_check.passed();
        commands.push(env_check);
        if !passed {
            return Ok(repo_report(
                "semantic_vector",
                scope.to_owned(),
                commands,
                cases,
                metrics,
                runtime_profile,
            ));
        }
        if suite
            .get("probe_provider_when_external")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            let mut probe = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    "semantic_vector_provider_probe",
                    vec![
                        runtime.binary.display().to_string(),
                        "provider".to_owned(),
                        "probe".to_owned(),
                        "--format".to_owned(),
                        "json".to_owned(),
                    ],
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let probe_passed = validate_provider_probe(&mut probe);
            metrics.push(MetricObservation {
                name: "semantic_vector_provider_probe_ms".to_owned(),
                value: probe.duration_ms as f64,
                budget: budget(suite, "provider_probe_budget_ms"),
                lower_is_better: true,
                key: true,
            });
            commands.push(probe);
            if !probe_passed {
                return Ok(repo_report(
                    "semantic_vector",
                    scope.to_owned(),
                    commands,
                    cases,
                    metrics,
                    runtime_profile,
                ));
            }
        }
    }
    for (index, evidence) in array_field(suite, "evidence").iter().enumerate() {
        let ingest = run_limited(
            &runtime.limiter,
            CommandSpec::new(
                format!("semantic_vector_ingest_{}", index + 1),
                semantic_vector_ingest_command(&runtime.binary, scope, evidence),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let passed = ingest.passed();
        commands.push(ingest);
        if !passed {
            return Ok(repo_report(
                "semantic_vector",
                scope.to_owned(),
                commands,
                cases,
                metrics,
                runtime_profile,
            ));
        }
    }
    let refresh = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            "semantic_vector_index_refresh",
            vec![
                runtime.binary.display().to_string(),
                "index".to_owned(),
                "refresh".to_owned(),
                "--kind".to_owned(),
                "semantic".to_owned(),
                "--kind".to_owned(),
                "vector".to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ],
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    metrics.push(MetricObservation {
        name: "semantic_vector_refresh_ms".to_owned(),
        value: refresh.duration_ms as f64,
        budget: budget(suite, "refresh_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    let refresh_passed = refresh.passed();
    commands.push(refresh);
    if !refresh_passed {
        return Ok(repo_report(
            "semantic_vector",
            scope.to_owned(),
            commands,
            cases,
            metrics,
            runtime_profile,
        ));
    }
    let results = parallel_map(
        array_field(suite, "query_cases").to_vec(),
        runtime.query_jobs.max(1),
        {
            let runtime = runtime.clone();
            let scope = scope.to_owned();
            move |case| {
                let query = run_limited(
                    &runtime.limiter,
                    CommandSpec::new(
                        format!("semantic_vector_{}", string_or(&case, "id", "case")),
                        semantic_vector_query_command(&runtime.binary, &scope, &case),
                        &runtime.workspace,
                        Some(runtime.env.clone()),
                        runtime.timeout,
                    ),
                );
                let observation = score_semantic_vector_case(&case, &query);
                (query, observation)
            }
        },
    );
    let durations = results
        .iter()
        .map(|(command, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation) in results {
        commands.push(command);
        cases.push(observation);
    }
    push_latency_metrics(&mut metrics, suite, "semantic_vector_query", &durations);
    Ok(repo_report(
        "semantic_vector",
        scope.to_owned(),
        commands,
        cases,
        metrics,
        runtime_profile,
    ))
}

struct JudgeEvalInput<'a> {
    workspace: &'a Path,
    run_home: &'a Path,
    env: &'a BTreeMap<String, String>,
    suite: &'a Value,
    generated_diff: bool,
    candidate_diff: &'a str,
    gates: &'a [GateObservation],
    cases: &'a [CaseObservation],
    metrics: &'a [MetricObservation],
    repo_reports: &'a [RepoReport],
    limiter: &'a Limiter,
}

fn evaluate_research_judge_suite(input: JudgeEvalInput<'_>) -> Result<RepoReport, String> {
    let settings = judge_settings(input.env);
    let mut report = repo_report(
        "research_judge",
        "judge".to_owned(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        settings_summary(&settings),
    );
    if !settings.enabled {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: true,
            duration_ms: 0,
            message: "judge skipped: backend disabled".to_owned(),
        });
        return Ok(report);
    }
    if let Some(error) = &settings.configuration_error {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: false,
            duration_ms: 0,
            message: format!("judge misconfigured: {error}"),
        });
        return Ok(report);
    }
    if !settings.missing.is_empty() {
        report.gates.push(GateObservation {
            name: "research_judge".to_owned(),
            passed: false,
            duration_ms: 0,
            message: format!(
                "judge misconfigured: missing {}",
                settings.missing.join(", ")
            ),
        });
        return Ok(report);
    }
    let prompt = build_judge_prompt(JudgePromptInput {
        workspace: input.workspace,
        suite: input.suite,
        generated_diff: input.generated_diff,
        candidate_diff: input.candidate_diff,
        gates: input.gates,
        cases: input.cases,
        metrics: input.metrics,
        repo_reports: input.repo_reports,
    });
    let prompt_file = input.run_home.join("judge-prompt.txt");
    fs::write(&prompt_file, &prompt)
        .map_err(|error| format!("failed to write {}: {error}", prompt_file.display()))?;
    let result = run_judge_backend(&input, &settings, &prompt_file, &prompt)?;
    let outcome = if result.passed() {
        judge_outcome(
            &format!("{}\n{}", result.stdout, result.stderr),
            input.suite,
        )
    } else {
        (false, false, 0.0, result.gate_message(), Value::Null)
    };
    report.gates.push(GateObservation {
        name: "research_judge".to_owned(),
        passed: outcome.0,
        duration_ms: result.duration_ms,
        message: outcome.3.clone(),
    });
    report.cases.push(CaseObservation {
        case_id: "research_judge".to_owned(),
        repository: "research_judge".to_owned(),
        passed: outcome.1,
        rank: outcome.1.then_some(1),
        max_rank: 1,
        false_positive_count: 0,
        message: outcome.3,
        objective: "research_judge".to_owned(),
        score_override: Some(outcome.2),
    });
    report.index_summary = outcome.4;
    report.commands.push(result);
    Ok(report)
}

struct FinishInput<'a> {
    config: &'a Config,
    generated_diff: bool,
    gates: Vec<GateObservation>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
    commands: Vec<CommandResult>,
    repo_reports: Vec<RepoReport>,
    run_home: PathBuf,
    job_plan: JobPlan,
}

fn finish(input: FinishInput<'_>) -> Result<EvaluationRun, String> {
    if input.run_home.exists() && !input.config.keep_workdirs {
        fs::remove_dir_all(&input.run_home)
            .map_err(|error| format!("failed to remove {}: {error}", input.run_home.display()))?;
    }
    let observation = EvaluationObservation {
        gates: input.gates,
        cases: input.cases,
        metrics: input.metrics,
        generated_diff: input.generated_diff,
    };
    let report = serde_json::json!({
        "profile": input.config.profile,
        "generated_diff": input.generated_diff,
        "parallelism": {
            "requested_jobs": input.config.jobs.label(),
            "requested_repo_jobs": input.config.repo_jobs.label(),
            "requested_query_jobs": input.config.query_jobs.label(),
            "global_jobs": input.job_plan.global,
            "repo_jobs": input.job_plan.repositories,
            "query_jobs": input.job_plan.queries,
        },
        "gates": observation.gates,
        "cases": observation.cases,
        "metrics": observation.metrics,
        "commands": input.commands.iter().map(CommandResult::serializable).collect::<Vec<_>>(),
        "repositories": input.repo_reports.iter().map(serializable_repo_report).collect::<Vec<_>>(),
    });
    Ok(EvaluationRun {
        observation,
        report,
    })
}

fn run_quality_gate_stages(
    profile: &str,
    workspace: &Path,
    limiter: &Limiter,
    commands: &mut Vec<CommandResult>,
    gates: &mut Vec<GateObservation>,
    metrics: &mut Vec<MetricObservation>,
) -> bool {
    for stage in quality_gate_stages(profile) {
        let mut stage_passed = true;
        for result in run_quality_gate_stage(stage, workspace, limiter) {
            metrics.push(MetricObservation {
                name: format!("{}_ms", result.name),
                value: result.duration_ms as f64,
                budget: quality_budget_ms(&result.name),
                lower_is_better: true,
                key: result.name == "cargo_build_release",
            });
            gates.push(GateObservation::from_command(&result));
            stage_passed &= result.passed();
            commands.push(result);
        }
        if !stage_passed {
            return false;
        }
    }
    true
}

fn run_quality_gate_stage(
    stage: QualityGateStage,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    match stage {
        QualityGateStage::Parallel(gates) => run_parallel_quality_gates(gates, workspace, limiter),
        QualityGateStage::Rails(rails) => run_quality_gate_rails(rails, workspace, limiter),
    }
}

fn run_parallel_quality_gates(
    gates: Vec<QualityGate>,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    let jobs = gates.len();
    let workspace = workspace.to_path_buf();
    let limiter = limiter.clone();
    let mut indexed_results = parallel_map(
        gates.into_iter().enumerate().collect(),
        jobs,
        move |(index, gate)| {
            let result = run_limited(
                &limiter,
                CommandSpec::new(
                    gate.name,
                    gate.command,
                    &workspace,
                    None,
                    gate.timeout_seconds,
                ),
            );
            (index, result)
        },
    );
    indexed_results.sort_by_key(|(index, _)| *index);
    indexed_results
        .into_iter()
        .map(|(_, result)| result)
        .collect()
}

fn run_quality_gate_rails(
    rails: Vec<Vec<QualityGate>>,
    workspace: &Path,
    limiter: &Limiter,
) -> Vec<CommandResult> {
    let jobs = rails.len();
    let workspace = workspace.to_path_buf();
    let limiter = limiter.clone();
    let mut indexed_rails = parallel_map(
        rails.into_iter().enumerate().collect(),
        jobs,
        move |(rail_index, rail)| {
            let mut rail_results = Vec::new();
            for gate in rail {
                let result = run_limited(
                    &limiter,
                    CommandSpec::new(
                        gate.name,
                        gate.command,
                        &workspace,
                        None,
                        gate.timeout_seconds,
                    ),
                );
                let passed = result.passed();
                rail_results.push(result);
                if !passed {
                    break;
                }
            }
            (rail_index, rail_results)
        },
    );
    indexed_rails.sort_by_key(|(rail_index, _)| *rail_index);
    indexed_rails
        .into_iter()
        .flat_map(|(_, results)| results)
        .collect()
}

fn run_limited(limiter: &Limiter, spec: CommandSpec) -> CommandResult {
    let _permit = limiter.acquire();
    run_command(&spec)
}

fn run_writer_limited(runtime: &EvalRuntime, spec: CommandSpec) -> CommandResult {
    let _permit = runtime.limiter.acquire();
    let _writer = runtime
        .writer_lock
        .lock()
        .expect("writer lock should not be poisoned");
    run_command(&spec)
}

fn parallel_map<T, R, F>(items: Vec<T>, jobs: usize, f: F) -> Vec<R>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> R + Send + Sync + 'static,
{
    if items.is_empty() {
        return Vec::new();
    }
    let queue = Arc::new(Mutex::new(items.into_iter().collect::<Vec<_>>()));
    let output = Arc::new(Mutex::new(Vec::new()));
    let function = Arc::new(f);
    let workers = jobs.max(1).min(queue.lock().expect("queue").len());
    let mut handles = Vec::new();
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let output = Arc::clone(&output);
        let function = Arc::clone(&function);
        handles.push(std::thread::spawn(move || {
            loop {
                let item = queue.lock().expect("queue").pop();
                let Some(item) = item else {
                    break;
                };
                let result = function(item);
                output.lock().expect("output").push(result);
            }
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }
    match Arc::try_unwrap(output) {
        Ok(output) => output.into_inner().expect("output should not be poisoned"),
        Err(_) => Vec::new(),
    }
}

include!("evaluator_repo_set.rs");
include!("evaluator_tail.rs");
