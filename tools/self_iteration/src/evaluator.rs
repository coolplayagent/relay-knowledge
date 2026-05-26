use std::{
    collections::{BTreeMap, BTreeSet},
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
    config::{CategorySet, Config, EvaluationCategory, JobPlan},
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
struct RegistrationCaseReport {
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    gates: Vec<GateObservation>,
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
    let evaluation_started = Instant::now();
    let job_plan = JobPlan::resolve(config);
    let limiter = Limiter::new(job_plan.global);
    let (run_home, cached_home) = evaluation_home(config, paths, run_id);
    eprintln!(
        "[self-iterate] evaluation start run_id={} profile={} home={} cached_home={} jobs=global:{},repo:{},query:{}",
        run_id,
        config.profile,
        run_home.display(),
        cached_home,
        job_plan.global,
        job_plan.repositories,
        job_plan.queries
    );
    if run_home.exists() && !config.keep_workdirs && !cached_home {
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
    let selection = WorkloadSelection::new(config);

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
            cached_home,
            job_plan,
            selection,
            started: evaluation_started,
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
            cached_home,
            job_plan,
            selection,
            started: evaluation_started,
        });
    }

    let binary = relay_knowledge_binary(config);
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
    if selection.runs_repository_workload(&config.profile) {
        let registration_report = evaluate_registration_cases(
            &runtime,
            &run_home,
            &repository_configs,
            cases_config,
            &config.profile,
            config.categories.as_ref(),
        )?;
        commands.extend(registration_report.commands);
        cases.extend(registration_report.cases);
        gates.extend(registration_report.gates);
    }
    let required_repo_set_members = if selection.runs_repository_sets(&config.profile) {
        selected_repository_set_member_names(
            cases_config,
            &config.profile,
            config.categories.as_ref(),
        )
    } else {
        BTreeSet::new()
    };
    if selection.runs_repository_workload(&config.profile) {
        let repositories = repository_configs
            .iter()
            .filter_map(|(name, repo_config)| {
                let needed_for_repo_set = required_repo_set_members.contains(name.as_str());
                if !needed_for_repo_set
                    && !repository_in_profile(&config.profile, name, repo_config)
                {
                    return None;
                }
                let repo_cases = grouped_cases
                    .get(name)
                    .cloned()
                    .map(|cases| {
                        select_repository_cases_for_profile(
                            &config.profile,
                            config.categories.as_ref(),
                            cases,
                        )
                    })
                    .unwrap_or_default();
                if repo_cases.is_empty() && !needed_for_repo_set {
                    return None;
                }
                Some((name.clone(), repo_config.clone(), repo_cases))
            })
            .collect::<Vec<_>>();
        let repo_jobs = job_plan.repositories.min(job_plan.global).max(1);
        let repository_case_count = repositories
            .iter()
            .map(|(_, _, repo_cases)| repo_cases.len())
            .sum::<usize>();
        eprintln!(
            "[self-iterate] repository workload start repositories={} query_cases={} repo_jobs={} query_jobs={}",
            repositories.len(),
            repository_case_count,
            repo_jobs,
            runtime.query_jobs
        );
        let repo_results = parallel_map(repositories, repo_jobs, {
            let runtime = runtime.clone();
            let run_home = run_home.clone();
            move |(repo_name, repo_config, repo_cases)| {
                evaluate_repository(&runtime, &run_home, &repo_name, &repo_config, repo_cases)
            }
        });
        for report in repo_results {
            let report = report?;
            commands.extend(report.commands.clone());
            gates.extend(report.commands.iter().map(GateObservation::from_command));
            gates.extend(report.gates.clone());
            cases.extend(report.cases.clone());
            metrics.extend(report.metrics.clone());
            repo_reports.push(report);
        }
        eprintln!(
            "[self-iterate] repository workload done reports={} commands={} cases={}",
            repo_reports.len(),
            commands.len(),
            cases.len()
        );
    }

    if selection.runs_repository_sets(&config.profile) {
        eprintln!(
            "[self-iterate] repository-set workload start profile={}",
            config.profile
        );
        for report in evaluate_repository_sets(
            &runtime,
            cases_config,
            &repository_configs,
            &config.profile,
            config.categories.as_ref(),
        )? {
            commands.extend(report.commands.clone());
            gates.extend(report.commands.iter().map(GateObservation::from_command));
            gates.extend(report.gates.clone());
            cases.extend(report.cases.clone());
            metrics.extend(report.metrics.clone());
            repo_reports.push(report);
        }
        eprintln!(
            "[self-iterate] repository-set workload done reports={} commands={} cases={}",
            repo_reports.len(),
            commands.len(),
            cases.len()
        );
    }

    if selection.runs_file_fixtures(&config.profile) {
        eprintln!("[self-iterate] file fixture workload start");
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
        eprintln!(
            "[self-iterate] file fixture workload done commands={} cases={} metrics={}",
            commands.len(),
            cases.len(),
            metrics.len()
        );
    }

    if selection.runs_semantic_vector(&config.profile) {
        if let Some(suite) = cases_config
            .get("semantic_vector_suite")
            .and_then(Value::as_object)
        {
            eprintln!("[self-iterate] semantic/vector workload start");
            let report = evaluate_semantic_vector_suite(
                &runtime,
                &semantic_vector_suite_for_selection(
                    &Value::Object(suite.clone()),
                    &config.profile,
                    config.categories.as_ref(),
                ),
            )?;
            commands.extend(report.commands.clone());
            gates.extend(report.commands.iter().map(GateObservation::from_command));
            gates.extend(report.gates.clone());
            cases.extend(report.cases.clone());
            metrics.extend(report.metrics.clone());
            repo_reports.push(report);
            eprintln!(
                "[self-iterate] semantic/vector workload done commands={} cases={} metrics={}",
                commands.len(),
                cases.len(),
                metrics.len()
            );
        }
    }

    if selection.runs_research_judge(&config.profile) {
        if let Some(suite) = cases_config
            .get("research_judge_suite")
            .and_then(Value::as_object)
        {
            eprintln!("[self-iterate] research judge workload start");
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
            eprintln!(
                "[self-iterate] research judge workload done gates={} cases={} metrics={}",
                gates.len(),
                cases.len(),
                metrics.len()
            );
        }
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
        cached_home,
        job_plan,
        selection,
        started: evaluation_started,
    })
}

fn evaluate_repository(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
    repo_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let alias = string_or(repo_config, "alias", repo_name);
    let ref_selector = string_or(repo_config, "ref", "HEAD");
    let scope = string_or(repo_config, "scope", "all").to_owned();
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut guardrail_gates = Vec::new();
    let mut metrics = Vec::new();
    let (path, setup_commands) =
        prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
    let setup_passed = setup_commands.iter().all(CommandResult::passed);
    commands.extend(setup_commands);
    eprintln!(
        "[self-iterate] repository start name={} alias={} path={} scope={} query_cases={}",
        repo_name,
        alias,
        path.display(),
        scope,
        repo_cases.len()
    );
    if !setup_passed {
        return Ok(repo_report(
            repo_name,
            scope,
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
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
            let duration_ms = query.duration_ms;
            let observation = score_query_case(&repo_name, &case, &query);
            let guardrail_gate = guardrail_gate_from_case(&observation, duration_ms);
            (query, observation, guardrail_gate)
        }
    });
    let query_durations = query_results
        .iter()
        .map(|(command, _, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation, guardrail_gate) in query_results {
        commands.push(command);
        cases.push(observation);
        if let Some(gate) = guardrail_gate {
            guardrail_gates.push(gate);
        }
    }
    push_latency_metrics(
        &mut metrics,
        repo_config,
        &format!("{repo_name}_query"),
        &query_durations,
    );
    let mut report = repo_report(repo_name, scope, commands, cases, metrics, index_json);
    report.gates = guardrail_gates;
    Ok(report)
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

fn evaluate_semantic_vector_suite(
    runtime: &EvalRuntime,
    suite: &Value,
) -> Result<RepoReport, String> {
    let scope = string_or(suite, "source_scope", "self-iteration-semantic-vector");
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut guardrail_gates = Vec::new();
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
                let duration_ms = query.duration_ms;
                let observation = score_semantic_vector_case(&case, &query);
                let guardrail_gate = guardrail_gate_from_case(&observation, duration_ms);
                (query, observation, guardrail_gate)
            }
        },
    );
    let durations = results
        .iter()
        .map(|(command, _, _)| command.duration_ms)
        .collect::<Vec<_>>();
    for (command, observation, guardrail_gate) in results {
        commands.push(command);
        cases.push(observation);
        if let Some(gate) = guardrail_gate {
            guardrail_gates.push(gate);
        }
    }
    push_latency_metrics(&mut metrics, suite, "semantic_vector_query", &durations);
    let mut report = repo_report(
        "semantic_vector",
        scope.to_owned(),
        commands,
        cases,
        metrics,
        runtime_profile,
    );
    report.gates = guardrail_gates;
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
    cached_home: bool,
    job_plan: JobPlan,
    selection: WorkloadSelection,
    started: Instant,
}

fn finish(input: FinishInput<'_>) -> Result<EvaluationRun, String> {
    if input.run_home.exists() && !input.config.keep_workdirs && !input.cached_home {
        fs::remove_dir_all(&input.run_home)
            .map_err(|error| format!("failed to remove {}: {error}", input.run_home.display()))?;
    }
    let observation = EvaluationObservation {
        gates: input.gates,
        cases: input.cases,
        metrics: input.metrics,
        generated_diff: input.generated_diff,
    };
    let passed_gates = observation.gates.iter().filter(|gate| gate.passed).count();
    let passed_cases = observation.cases.iter().filter(|case| case.passed).count();
    eprintln!(
        "[self-iterate] evaluation done profile={} duration_ms={} gates={}/{} cases={}/{} commands={} metrics={}",
        input.config.profile,
        input.started.elapsed().as_millis(),
        passed_gates,
        observation.gates.len(),
        passed_cases,
        observation.cases.len(),
        input.commands.len(),
        observation.metrics.len()
    );
    let report = serde_json::json!({
        "profile": input.config.profile,
        "selected_categories": input.selection.selected_categories_report(),
        "generated_diff": input.generated_diff,
        "evaluation_home": input.run_home.display().to_string(),
        "cached_home": input.cached_home,
        "skipped_suites": input.selection.skipped_suites(&input.config.profile),
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
    let stages = quality_gate_stages(profile);
    let stage_count = stages.len();
    for (stage_index, stage) in stages.into_iter().enumerate() {
        let stage_started = Instant::now();
        let stage_label = quality_gate_stage_label(&stage);
        eprintln!(
            "[self-iterate] quality stage {}/{} start {}",
            stage_index + 1,
            stage_count,
            stage_label
        );
        let mut stage_passed = true;
        let mut stage_gate_count = 0usize;
        for result in run_quality_gate_stage(stage, workspace, limiter) {
            stage_gate_count += 1;
            metrics.push(MetricObservation {
                name: format!("{}_ms", result.name),
                value: result.duration_ms as f64,
                budget: quality_budget_ms(&result.name),
                lower_is_better: true,
                key: matches!(
                    result.name.as_str(),
                    "cargo_build_release" | "cargo_build_debug"
                ),
            });
            gates.push(GateObservation::from_command(&result));
            stage_passed &= result.passed();
            commands.push(result);
        }
        eprintln!(
            "[self-iterate] quality stage {}/{} done passed={} duration_ms={} gates={}",
            stage_index + 1,
            stage_count,
            stage_passed,
            stage_started.elapsed().as_millis(),
            stage_gate_count
        );
        if !stage_passed {
            eprintln!("[self-iterate] quality gates failed; skipping evaluation workload");
            return false;
        }
    }
    true
}

fn quality_gate_stage_label(stage: &QualityGateStage) -> String {
    match stage {
        QualityGateStage::Parallel(gates) => {
            format!("parallel gates={}", quality_gate_names(gates))
        }
        QualityGateStage::Rails(rails) => {
            let rails = rails
                .iter()
                .enumerate()
                .map(|(index, rail)| format!("rail{}={}", index + 1, quality_gate_names(rail)))
                .collect::<Vec<_>>()
                .join("; ");
            format!("rails {rails}")
        }
    }
}

fn quality_gate_names(gates: &[QualityGate]) -> String {
    gates
        .iter()
        .map(|gate| gate.name)
        .collect::<Vec<_>>()
        .join(",")
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
