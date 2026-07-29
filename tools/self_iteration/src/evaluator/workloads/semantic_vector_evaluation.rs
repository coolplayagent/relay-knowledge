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
