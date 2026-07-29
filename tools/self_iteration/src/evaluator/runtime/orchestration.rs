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

    let cli_contract_report = evaluate_cli_contract_cases(
        &runtime,
        &run_home,
        cases_config,
        &config.profile,
        config.categories.as_ref(),
    );
    commands.extend(cli_contract_report.commands);
    cases.extend(cli_contract_report.cases);
    gates.extend(cli_contract_report.gates);

    let query_cases = array_field(cases_config, "query_cases");
    let software_query_cases = array_field(cases_config, "software_query_cases");
    let grouped_cases = objects_by_repository(query_cases);
    let grouped_software_cases = objects_by_repository(software_query_cases);
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
                let software_cases = grouped_software_cases
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
                if repo_cases.is_empty() && software_cases.is_empty() && !needed_for_repo_set {
                    return None;
                }
                Some((
                    name.clone(),
                    repo_config.clone(),
                    repo_cases,
                    software_cases,
                ))
            })
            .collect::<Vec<_>>();
        let repo_jobs = job_plan.repositories.min(job_plan.global).max(1);
        let repository_case_count = repositories
            .iter()
            .map(|(_, _, repo_cases, software_cases)| repo_cases.len() + software_cases.len())
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
            move |(repo_name, repo_config, repo_cases, software_cases)| {
                evaluate_repository(
                    &runtime,
                    &run_home,
                    &repo_name,
                    &repo_config,
                    repo_cases,
                    software_cases,
                )
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

    if selection.runs_agent_workflows(&config.profile) {
        eprintln!(
            "[self-iterate] agent workflow workload start profile={}",
            config.profile
        );
        for report in evaluate_agent_workflows(
            &runtime,
            &run_home,
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
            "[self-iterate] agent workflow workload done reports={} commands={} cases={} metrics={}",
            repo_reports.len(),
            commands.len(),
            cases.len(),
            metrics.len()
        );
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
