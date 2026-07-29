fn create_file_fixture(root: &Path, fixture: &Value) -> Result<(), String> {
    if root.exists() {
        fs::remove_dir_all(root)
            .map_err(|error| format!("failed to remove {}: {error}", root.display()))?;
    }
    fs::create_dir_all(root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    for file in array_field(fixture, "files") {
        write_fixture_file(
            &root.join(string_or(file, "path", "fixture.txt")),
            string_or(file, "content", "fixture"),
        )?;
    }
    for index in 0..number_or(fixture, "generate_noise_files", 0) {
        write_fixture_file(
            &root
                .join("noise")
                .join(format!("quarterly-design-noise-{index:04}.txt")),
            &format!("noise {index}"),
        )?;
    }
    Ok(())
}

fn file_fixture_env(env: &BTreeMap<String, String>, root: &Path) -> BTreeMap<String, String> {
    let mut fixture_env = env.clone();
    let root_value = root.display().to_string();
    let mut roots: Vec<String> = fixture_env
        .get("RELAY_KNOWLEDGE_FILE_INDEX_ROOTS")
        .map(|value| {
            value
                .split(';')
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if !roots.iter().any(|value| value == &root_value) {
        roots.push(root_value);
    }
    fixture_env.insert(
        "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS".to_owned(),
        roots.join(";"),
    );
    fixture_env
}

fn background_file_env(
    env: &BTreeMap<String, String>,
    root: &Path,
    scan_interval_ms: u64,
) -> BTreeMap<String, String> {
    let mut fixture_env = file_fixture_env(env, root);
    fixture_env.insert(
        "RELAY_KNOWLEDGE_FILE_INDEX_ENABLED".to_owned(),
        "true".to_owned(),
    );
    fixture_env.insert(
        "RELAY_KNOWLEDGE_FILE_INDEX_SCAN_INTERVAL_MS".to_owned(),
        scan_interval_ms.to_string(),
    );
    fixture_env
        .entry("RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS".to_owned())
        .or_insert_with(|| "5000".to_owned());
    fixture_env
        .entry("RELAY_KNOWLEDGE_FILE_INDEX_QUERY_TIMEOUT_MS".to_owned())
        .or_insert_with(|| "750".to_owned());
    fixture_env
}

fn file_query_command(binary: &Path, case: &Value) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "files".to_owned(),
        "query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--source".to_owned(),
        "local-files".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 10).to_string(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn score_file_case(fixture_name: &str, case: &Value, result: &CommandResult) -> CaseObservation {
    let objective = string_or(case, "objective", "competitive_capability").to_owned();
    if !result.passed() {
        return failed_case(case, fixture_name, &objective, result);
    }
    let payload = match parse_json_case_output(case, fixture_name, &objective, result) {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let max_rank = number_or(case, "max_rank", 1) as usize;
    let assessment = assess_ranked_hits(case, hits, expected, forbidden);
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
        repository: fixture_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
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

fn payload_constraint_failures(case: &Value, payload: &Value, results_len: usize) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(max_results) = case.get("max_results").and_then(Value::as_u64) {
        if results_len > max_results as usize {
            failures.push(format!("results={results_len} max_results={max_results}"));
        }
    }
    if let Some(expected) = case.get("truncated").and_then(Value::as_bool) {
        let actual = payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if actual != expected {
            failures.push(format!("truncated={actual} expected={expected}"));
        }
    }
    if case.get("degraded_reason").is_some() {
        let actual = payload.get("degraded_reason").and_then(Value::as_str);
        match case.get("degraded_reason").expect("checked above") {
            Value::Null if actual.is_some() => {
                failures.push(format!("degraded_reason={}", actual.unwrap_or_default()));
            }
            Value::Bool(false) if actual.is_some() => {
                failures.push(format!("degraded_reason={}", actual.unwrap_or_default()));
            }
            Value::String(expected) if actual != Some(expected.as_str()) => {
                failures.push(format!(
                    "degraded_reason={} expected={expected}",
                    actual.unwrap_or("missing")
                ));
            }
            _ => {}
        }
    }
    if let Some(expected) = case
        .get("degraded_reason_contains")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let actual = payload
            .get("degraded_reason")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !actual.contains(expected) {
            failures.push(format!("degraded_reason={actual} missing={expected}"));
        }
    }
    failures
}

fn evaluate_background_file_case(
    runtime: &EvalRuntime,
    fixture_root: &Path,
    cases_config: &Value,
    case: &Value,
) -> Result<(CommandResult, CaseObservation, MetricObservation), String> {
    let fixture_name = string_or(case, "fixture", "");
    let fixture = object_field(cases_config, "file_fixtures")
        .and_then(|fixtures| fixtures.get(fixture_name))
        .ok_or_else(|| format!("missing fixture {fixture_name}"))?;
    let root = fixture_root.join(format!(
        "{}-{}",
        fixture_name,
        string_or(case, "id", "case")
    ));
    create_file_fixture(&root, fixture)?;
    let started = Instant::now();
    let fixture_env = background_file_env(
        &runtime.env,
        &root,
        number_or(case, "scan_interval_ms", 250),
    );
    eprintln!(
        "[self-iterate] background file fixture service start fixture={} case={} timeout_s={}",
        fixture_name,
        string_or(case, "id", "case"),
        runtime.timeout.min(number_or(case, "timeout_seconds", 8))
    );
    let mut service = Command::new(&runtime.binary)
        .args(["service", "run"])
        .current_dir(&runtime.workspace)
        .env_clear()
        .envs(&fixture_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start background service: {error}"))?;
    for action in array_field(case, "actions_after_start") {
        apply_fixture_action(&root, action)?;
    }
    let deadline = Instant::now()
        + std::time::Duration::from_secs(
            runtime.timeout.min(number_or(case, "timeout_seconds", 8)),
        );
    let mut final_query = None;
    while Instant::now() < deadline {
        if service
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        let query = run_command(&CommandSpec::new(
            format!("{}_{}_query", fixture_name, string_or(case, "id", "case")),
            file_query_command(&runtime.binary, case),
            &runtime.workspace,
            Some(fixture_env.clone()),
            5,
        ));
        let observation = score_file_case(fixture_name, case, &query);
        let passed = observation.passed;
        final_query = Some(query);
        if passed {
            break;
        }
        eprintln!(
            "[self-iterate] background file fixture polling fixture={} case={} elapsed_ms={}",
            fixture_name,
            string_or(case, "id", "case"),
            started.elapsed().as_millis()
        );
        std::thread::sleep(std::time::Duration::from_millis(number_or(
            case,
            "poll_interval_ms",
            200,
        )));
    }
    let _ = service.kill();
    let _ = service.wait();
    let duration_ms = started.elapsed().as_millis() as u64;
    eprintln!(
        "[self-iterate] background file fixture service done fixture={} case={} duration_ms={}",
        fixture_name,
        string_or(case, "id", "case"),
        duration_ms
    );
    let query = final_query.unwrap_or(CommandResult {
        name: format!("{}_{}_query", fixture_name, string_or(case, "id", "case")),
        command: file_query_command(&runtime.binary, case),
        exit_code: 1,
        duration_ms,
        stdout: String::new(),
        stderr: "background file index service exited before query".to_owned(),
    });
    let observation = score_file_case(fixture_name, case, &query);
    Ok((
        query,
        observation,
        MetricObservation {
            name: format!(
                "{}_{}_file_auto_index_first_seen_ms",
                fixture_name,
                string_or(case, "id", "case")
            ),
            value: duration_ms as f64,
            budget: budget(case, "auto_index_budget_ms"),
            lower_is_better: true,
            key: true,
        },
    ))
}

fn apply_fixture_action(root: &Path, action: &Value) -> Result<(), String> {
    match string_or(action, "type", "") {
        "write" => write_fixture_file(
            &root.join(string_or(action, "path", "fixture.txt")),
            string_or(action, "content", "fixture"),
        ),
        other => Err(format!("unsupported fixture action: {other}")),
    }
}
