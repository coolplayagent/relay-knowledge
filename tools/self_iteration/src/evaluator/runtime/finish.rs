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
