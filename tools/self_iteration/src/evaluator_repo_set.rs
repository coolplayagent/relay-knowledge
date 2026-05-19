fn evaluate_repository_sets(
    runtime: &EvalRuntime,
    cases_config: &Value,
    repositories: &BTreeMap<String, Value>,
    profile: &str,
) -> Result<Vec<RepoReport>, String> {
    let set_configs = object_field(cases_config, "repository_sets")
        .map(|object| {
            object
                .iter()
                .map(|(name, config)| (name.clone(), config.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let cases_by_set = repository_set_cases_by_name(array_field(
        cases_config,
        "repository_set_query_cases",
    ));
    let mut reports = Vec::new();
    for (set_name, set_config) in set_configs {
        if set_config.get("profile").and_then(Value::as_str) == Some("exhaustive")
            && profile != "exhaustive"
        {
            continue;
        }
        let set_cases = cases_by_set.get(&set_name).cloned().unwrap_or_default();
        if set_cases.is_empty() {
            continue;
        }
        reports.push(evaluate_repository_set(
            runtime,
            &set_name,
            &set_config,
            repositories,
            set_cases,
        )?);
    }
    Ok(reports)
}

fn repository_set_cases_by_name(cases: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for case in cases {
        if let Some(repository_set) = string_field(case, "repository_set") {
            grouped
                .entry(repository_set.to_owned())
                .or_default()
                .push(case.clone());
        }
    }
    grouped
}

fn evaluate_repository_set(
    runtime: &EvalRuntime,
    set_name: &str,
    set_config: &Value,
    repositories: &BTreeMap<String, Value>,
    set_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let set_alias = string_or(set_config, "alias", set_name);
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut metrics = Vec::new();
    let create = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{set_name}_repo_set_create"),
            repo_set_create_command(&runtime.binary, set_alias, set_config),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    let create_passed = create.passed();
    commands.push(create);
    if !create_passed {
        return Ok(repo_report(
            set_name,
            set_alias.to_owned(),
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }
    for member in array_field(set_config, "members") {
        let member_repository = string_or(member, "repository", "");
        let Some(repo_config) = repositories.get(member_repository) else {
            commands.push(repository_set_validation_command(
                set_name,
                format!("missing repository_set member repository: {member_repository}"),
            ));
            return Ok(repo_report(
                set_name,
                set_alias.to_owned(),
                commands,
                cases,
                metrics,
                Value::Null,
            ));
        };
        let repository_alias = string_or(repo_config, "alias", member_repository);
        let ref_selector = string_field(member, "ref")
            .or_else(|| string_field(repo_config, "ref"))
            .unwrap_or("HEAD");
        let add = run_limited(
            &runtime.limiter,
            CommandSpec::new(
                format!("{set_name}_{member_repository}_repo_set_add"),
                repo_set_add_command(
                    &runtime.binary,
                    set_alias,
                    repository_alias,
                    ref_selector,
                    member,
                ),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let add_passed = add.passed();
        commands.push(add);
        if !add_passed {
            return Ok(repo_report(
                set_name,
                set_alias.to_owned(),
                commands,
                cases,
                metrics,
                Value::Null,
            ));
        }
    }
    let refresh = run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{set_name}_repo_set_refresh"),
            repo_set_refresh_command(&runtime.binary, set_alias),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    let refresh_json = parse_json_output(&refresh.stdout);
    metrics.push(MetricObservation {
        name: format!("{set_name}_repo_set_refresh_ms"),
        value: refresh.duration_ms as f64,
        budget: budget(set_config, "refresh_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    let refresh_passed = refresh.passed();
    commands.push(refresh);
    if !refresh_passed {
        return Ok(repo_report(
            set_name,
            set_alias.to_owned(),
            commands,
            cases,
            metrics,
            refresh_json,
        ));
    }
    let query_results = parallel_map(set_cases, runtime.query_jobs.max(1), {
        let runtime = runtime.clone();
        let set_alias = set_alias.to_owned();
        let set_name = set_name.to_owned();
        move |case| {
            let query = run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{}_{}", set_name, string_or(&case, "id", "case")),
                    repo_set_query_command(&runtime.binary, &set_alias, &case),
                    &runtime.workspace,
                    Some(runtime.env.clone()),
                    runtime.timeout,
                ),
            );
            let observation = score_repository_set_case(&set_name, &case, &query);
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
        set_config,
        &format!("{set_name}_repo_set_query"),
        &query_durations,
    );
    Ok(repo_report(
        set_name,
        set_alias.to_owned(),
        commands,
        cases,
        metrics,
        refresh_json,
    ))
}

fn repository_set_validation_command(set_name: &str, message: String) -> CommandResult {
    CommandResult {
        name: format!("{set_name}_repo_set_config"),
        command: vec!["validate".to_owned(), "repository-set-config".to_owned()],
        exit_code: 1,
        duration_ms: 0,
        stdout: String::new(),
        stderr: message,
    }
}

fn repo_set_create_command(binary: &Path, set_alias: &str, set_config: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo-set".to_owned(),
        "create".to_owned(),
        set_alias.to_owned(),
    ];
    if let Some(description) =
        string_field(set_config, "description").filter(|description| !description.is_empty())
    {
        command.extend(["--description".to_owned(), description.to_owned()]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn repo_set_add_command(
    binary: &Path,
    set_alias: &str,
    repository_alias: &str,
    ref_selector: &str,
    member: &Value,
) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo-set".to_owned(),
        "add".to_owned(),
        set_alias.to_owned(),
        repository_alias.to_owned(),
        "--ref".to_owned(),
        ref_selector.to_owned(),
        "--priority".to_owned(),
        number_or(member, "priority", 0).to_string(),
    ];
    for path in string_vec(member, "path_filters") {
        command.extend(["--path".to_owned(), path]);
    }
    for language in string_vec(member, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn repo_set_refresh_command(binary: &Path, set_alias: &str) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "repo-set".to_owned(),
        "refresh".to_owned(),
        set_alias.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn repo_set_query_command(binary: &Path, set_alias: &str, case: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo-set".to_owned(),
        "query".to_owned(),
        set_alias.to_owned(),
        "--query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--kind".to_owned(),
        string_or(case, "kind", "hybrid").to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 10).to_string(),
    ];
    for path in string_vec(case, "path_filters") {
        command.extend(["--path".to_owned(), path]);
    }
    for language in string_vec(case, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn score_repository_set_case(
    set_name: &str,
    case: &Value,
    result: &CommandResult,
) -> CaseObservation {
    let objective = string_or(case, "objective", "competitive_capability").to_owned();
    if !result.passed() {
        return failed_case(case, set_name, &objective, result);
    }
    let payload = match parse_json_case_output(case, set_name, &objective, result) {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = flatten_repository_set_hits(&payload);
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let max_rank = number_or(case, "max_rank", 1) as usize;
    let assessment = assess_ranked_hits(case, &hits, expected, forbidden);
    let mut failures = assessment.failures.clone();
    failures.extend(payload_constraint_failures(case, &payload, hits.len()));
    let mut passed = failures.is_empty();
    let mut rank = assessment.rank;
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        passed = hits.is_empty() && failures.is_empty();
        rank = passed.then_some(0);
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: set_name.to_owned(),
        passed,
        rank,
        max_rank,
        false_positive_count: assessment.false_positive_count,
        message: format!(
            "results={} rank={rank:?} {} {}",
            hits.len(),
            assessment.details,
            failures.join("; ")
        ),
        objective,
        score_override: Some(if passed { assessment.score } else { 0.0 }),
    }
}

fn flatten_repository_set_hits(payload: &Value) -> Vec<Value> {
    score_array_field(payload, "results")
        .iter()
        .map(|result| {
            let mut flattened = result
                .get("hit")
                .cloned()
                .unwrap_or_else(|| result.clone());
            let Some(map) = flattened.as_object_mut() else {
                return flattened;
            };
            if let Some(member) = result.get("member").and_then(Value::as_object) {
                for key in [
                    "repository_alias",
                    "repository_id",
                    "source_scope",
                    "resolved_commit_sha",
                ] {
                    if let Some(value) = member.get(key) {
                        map.insert(key.to_owned(), value.clone());
                    }
                }
            }
            if let Some(score) = result.get("score") {
                map.insert("repository_set_score".to_owned(), score.clone());
            }
            flattened
        })
        .collect()
}

#[cfg(test)]
mod repo_set_tests {
    use super::*;

    #[test]
    fn flattens_repository_set_member_provenance() {
        let payload = serde_json::json!({
            "results": [{
                "member": {
                    "repository_alias": "sdk",
                    "source_scope": "sdk::HEAD",
                    "resolved_commit_sha": "abc"
                },
                "hit": {
                    "path": "client/client.go",
                    "excerpt": "func Dial"
                },
                "score": 0.7
            }]
        });

        let hits = flatten_repository_set_hits(&payload);

        assert_eq!(hits[0]["repository_alias"], "sdk");
        assert_eq!(hits[0]["source_scope"], "sdk::HEAD");
        assert_eq!(hits[0]["path"], "client/client.go");
        assert_eq!(hits[0]["repository_set_score"], 0.7);
    }
}
