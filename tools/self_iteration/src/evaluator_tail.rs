fn quality_gate_stages(profile: &str) -> Vec<QualityGateStage> {
    if profile == "smoke" {
        return vec![
            QualityGateStage::Parallel(vec![
                quality_gate("cargo_fmt_check", ["cargo", "fmt", "--all", "--", "--check"], 120),
                quality_gate(
                    "self_iteration_cargo_fmt_check",
                    [
                        "cargo",
                        "fmt",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--",
                        "--check",
                    ],
                    120,
                ),
            ]),
        ];
    }
    if profile == "fast" {
        return vec![
            QualityGateStage::Parallel(vec![
                quality_gate("cargo_fmt_check", ["cargo", "fmt", "--all", "--", "--check"], 120),
                quality_gate(
                    "self_iteration_cargo_fmt_check",
                    [
                        "cargo",
                        "fmt",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--",
                        "--check",
                    ],
                    120,
                ),
            ]),
            QualityGateStage::Parallel(vec![quality_gate(
                "cargo_build_debug",
                ["cargo", "build", "--bin", "relay-knowledge"],
                600,
            )]),
            QualityGateStage::Parallel(vec![quality_gate(
                "self_iteration_cargo_check",
                [
                    "cargo",
                    "check",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--all-targets",
                ],
                180,
            )]),
        ];
    }
    vec![
        QualityGateStage::Parallel(vec![
            quality_gate("cargo_fmt_check", ["cargo", "fmt", "--all", "--", "--check"], 120),
            quality_gate(
                "self_iteration_cargo_fmt_check",
                [
                    "cargo",
                    "fmt",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--",
                    "--check",
                ],
                120,
            ),
        ]),
        QualityGateStage::Parallel(vec![
            quality_gate("cargo_build_release", ["cargo", "build", "--release"], 1200),
            quality_gate(
                "self_iteration_cargo_build_release",
                [
                    "cargo",
                    "build",
                    "--release",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--bin",
                    "relay-knowledge-self-iterate",
                ],
                300,
            ),
        ]),
        QualityGateStage::Rails(vec![
            vec![
                quality_gate(
                    "cargo_clippy",
                    [
                        "cargo",
                        "clippy",
                        "--all-targets",
                        "--all-features",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    1200,
                ),
                quality_gate(
                    "cargo_test",
                    ["cargo", "test", "--all-targets", "--all-features"],
                    1200,
                ),
            ],
            vec![
                quality_gate(
                    "self_iteration_cargo_clippy",
                    [
                        "cargo",
                        "clippy",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    300,
                ),
                quality_gate(
                    "self_iteration_cargo_test",
                    [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                    ],
                    300,
                ),
            ],
        ]),
    ]
}

fn quality_gate<const N: usize>(
    name: &'static str,
    command: [&'static str; N],
    timeout_seconds: u64,
) -> QualityGate {
    QualityGate {
        name,
        command: command.into_iter().map(ToOwned::to_owned).collect(),
        timeout_seconds,
    }
}

fn quality_budget_ms(name: &str) -> Option<f64> {
    match name {
        "cargo_build_debug" => Some(90_000.0),
        "self_iteration_cargo_check" => Some(30_000.0),
        "cargo_build_release" => Some(180_000.0),
        "self_iteration_cargo_build_release" => Some(60_000.0),
        "cargo_fmt_check" => Some(20_000.0),
        "self_iteration_cargo_fmt_check" => Some(20_000.0),
        "cargo_clippy" => Some(180_000.0),
        "self_iteration_cargo_clippy" => Some(60_000.0),
        "cargo_test" => Some(240_000.0),
        "self_iteration_cargo_test" => Some(60_000.0),
        _ => None,
    }
}

fn evaluation_home(config: &Config, paths: &HistoryPaths, run_id: &str) -> (PathBuf, bool) {
    if config.profile == "fast" {
        return (
            paths.root.join("cache-v2").join("fast-evaluation-home"),
            true,
        );
    }
    (paths.work.join(run_id).join("home"), false)
}

fn relay_knowledge_binary(config: &Config) -> PathBuf {
    config
        .workspace
        .join("target")
        .join(if config.profile == "fast" {
            "debug"
        } else {
            "release"
        })
        .join("relay-knowledge")
}

fn repository_in_profile(profile: &str, repo_name: &str, repo_config: &Value) -> bool {
    if repo_config.get("profile").and_then(Value::as_str) == Some("exhaustive")
        && profile != "exhaustive"
    {
        return false;
    }
    profile != "fast" || fast_repository_names().iter().any(|name| name == repo_name)
}

fn limit_cases_for_profile(profile: &str, cases: Vec<Value>) -> Vec<Value> {
    let Some(limit) = fast_case_limit(profile) else {
        return cases;
    };
    cases.into_iter().take(limit).collect()
}

fn profile_runs_slow_suites(profile: &str) -> bool {
    matches!(profile, "full" | "exhaustive")
}

fn profile_runs_repository_sets(profile: &str) -> bool {
    matches!(profile, "fast" | "full" | "exhaustive")
}

fn repository_set_in_profile(profile: &str, set_name: &str) -> bool {
    profile != "fast" || fast_repository_set_names().iter().any(|name| name == set_name)
}

fn limit_repository_set_cases_for_profile(profile: &str, cases: Vec<Value>) -> Vec<Value> {
    if profile != "fast" {
        return cases;
    }
    cases
        .into_iter()
        .take(fast_repository_set_case_limit())
        .collect()
}

fn skipped_suites_for_profile(profile: &str) -> Vec<&'static str> {
    if profile_runs_slow_suites(profile) {
        Vec::new()
    } else if profile == "smoke" {
        vec![
            "repository_evaluation",
            "repository_sets",
            "file_fixtures",
            "semantic_vector",
            "research_judge",
        ]
    } else {
        vec![
            "file_fixtures",
            "semantic_vector",
            "research_judge",
        ]
    }
}
fn fast_case_limit(profile: &str) -> Option<usize> {
    (profile == "fast").then(|| {
        std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(6)
        })
}

fn fast_repository_set_case_limit() -> usize {
    std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn fast_repository_names() -> Vec<String> {
    std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPOS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| {
            vec![
                "relay_teams".to_owned(),
                "leveldb_cpp".to_owned(),
                "temporal_samples_go".to_owned(),
                "temporal_sdk_go".to_owned(),
            ]
        })
}

fn fast_repository_set_names() -> Vec<String> {
    std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SETS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec!["temporal_go_workspace".to_owned()])
}

fn register_command(binary: &Path, path: &Path, alias: &str) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "register".to_owned(),
        path.display().to_string(),
        "--alias".to_owned(),
        alias.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn query_command(binary: &Path, alias: &str, ref_selector: &str, case: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "query".to_owned(),
        alias.to_owned(),
        "--query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--kind".to_owned(),
        string_or(case, "kind", "hybrid").to_owned(),
        "--ref".to_owned(),
        string_or(case, "ref", ref_selector).to_owned(),
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

fn score_query_case(repo_name: &str, case: &Value, result: &CommandResult) -> CaseObservation {
    let objective = repository_case_objective(case);
    if !result.passed() {
        return failed_case(case, repo_name, &objective, result);
    }
    let payload = match parse_json_case_output(case, repo_name, &objective, result) {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let mut assessment = assess_ranked_hits(case, hits, expected, forbidden);
    let mut rank = assessment.rank;
    let mut passed = assessment.failures.is_empty();
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        passed = hits.is_empty();
        rank = passed.then_some(0);
        assessment = RankedAssessment {
            rank,
            false_positive_count: 0,
            score: if passed { 1.0 } else { 0.0 },
            details: "expect_empty".to_owned(),
            failures: if passed {
                Vec::new()
            } else {
                vec![format!("expected_empty_results={}", hits.len())]
            },
        };
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        rank,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: assessment.false_positive_count,
        message: format!(
            "results={} rank={rank:?} {}",
            hits.len(),
            assessment.details
        ),
        objective,
        score_override: Some(assessment.score),
    }
}

fn repository_case_objective(case: &Value) -> String {
    if let Some(objective) = string_field(case, "objective").filter(|value| !value.is_empty()) {
        return objective.to_owned();
    }
    let kind = string_or(case, "kind", "").to_ascii_lowercase();
    let case_id = string_or(case, "id", "").to_ascii_lowercase();
    let competitive_kinds = ["hybrid", "callers", "callees"];
    let markers = [
        "hybrid",
        "fuzzy",
        "full_scope",
        "fanout",
        "callers",
        "callees",
    ];
    if competitive_kinds.contains(&kind.as_str())
        || markers.iter().any(|marker| case_id.contains(marker))
    {
        "competitive_capability".to_owned()
    } else {
        "foundational_capability".to_owned()
    }
}

fn failed_case(
    case: &Value,
    repository: &str,
    objective: &str,
    result: &CommandResult,
) -> CaseObservation {
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repository.to_owned(),
        passed: false,
        rank: None,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: 0,
        message: result.gate_message(),
        objective: objective.to_owned(),
        score_override: None,
    }
}

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

fn write_fixture_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("failed to write {}: {error}", path.display()))
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

fn semantic_vector_runtime_profile(env: &BTreeMap<String, String>) -> Value {
    let semantic_backend = normalized_env(env, "RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local");
    let vector_backend = normalized_env(env, "RELAY_KNOWLEDGE_VECTOR_BACKEND", "local");
    let external_requested = semantic_backend == "external" || vector_backend == "external";
    let required = [
        "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
        "RELAY_KNOWLEDGE_EMBEDDING_API_KEY",
        "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL",
        "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION",
    ];
    let missing = required
        .iter()
        .filter(|name| {
            external_requested
                && env
                    .get(**name)
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(true)
        })
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    serde_json::json!({
        "semantic_backend": semantic_backend,
        "vector_backend": vector_backend,
        "external_requested": external_requested,
        "missing_external_env": missing,
    })
}

fn semantic_vector_env_check(profile: &Value) -> CommandResult {
    let missing = profile
        .get("missing_external_env")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let passed = missing.is_empty();
    CommandResult {
        name: "semantic_vector_external_env".to_owned(),
        command: vec!["validate".to_owned(), "semantic-vector-env".to_owned()],
        exit_code: if passed { 0 } else { 1 },
        duration_ms: 0,
        stdout: profile.to_string(),
        stderr: if passed {
            String::new()
        } else {
            format!("missing external semantic/vector env: {missing:?}")
        },
    }
}

fn validate_provider_probe(result: &mut CommandResult) -> bool {
    if !result.passed() {
        return false;
    }
    let Some(payload) = parse_json_output_value(&result.stdout) else {
        result.exit_code = 1;
        result.stderr = "provider probe returned invalid JSON".to_owned();
        return false;
    };
    if payload.get("ok").and_then(Value::as_bool).unwrap_or(true) {
        return true;
    }
    result.exit_code = 1;
    result.stderr = payload
        .get("error")
        .or_else(|| payload.get("error_code"))
        .and_then(Value::as_str)
        .unwrap_or("provider probe reported ok=false")
        .to_owned();
    false
}

fn semantic_vector_ingest_command(binary: &Path, scope: &str, evidence: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "ingest".to_owned(),
        "--source".to_owned(),
        scope.to_owned(),
        "--content".to_owned(),
        string_or(evidence, "content", "").to_owned(),
    ];
    for entity in string_vec(evidence, "entities") {
        command.extend(["--entity".to_owned(), entity]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn semantic_vector_query_command(binary: &Path, scope: &str, case: &Value) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--source".to_owned(),
        scope.to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 10).to_string(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn score_semantic_vector_case(case: &Value, result: &CommandResult) -> CaseObservation {
    if !result.passed() {
        return failed_case(case, "semantic_vector", "semantic_vector", result);
    }
    let payload = match parse_json_case_output(case, "semantic_vector", "semantic_vector", result)
    {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let max_rank = number_or(case, "max_rank", 1) as usize;
    let rank = hits
        .iter()
        .enumerate()
        .find_map(|(index, hit)| hit_matches_any(hit, expected).then_some(index + 1));
    let false_positives = hits
        .iter()
        .filter(|hit| hit_matches_any(hit, forbidden))
        .count();
    let missing_sources =
        missing_required_sources(case, rank.and_then(|index| hits.get(index - 1)), hits);
    let missing_backends = missing_required_backends(case, &payload);
    let mut passed = (expected.is_empty() || rank.is_some_and(|rank| rank <= max_rank))
        && false_positives == 0
        && missing_sources.is_empty()
        && missing_backends.is_empty();
    let mut final_rank = rank;
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        passed = hits.is_empty();
        final_rank = passed.then_some(0);
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: "semantic_vector".to_owned(),
        passed,
        rank: final_rank,
        max_rank,
        false_positive_count: false_positives,
        message: format!(
            "results={} rank={final_rank:?} missing_sources={missing_sources:?} missing_backends={missing_backends:?}",
            hits.len()
        ),
        objective: "semantic_vector".to_owned(),
        score_override: None,
    }
}

fn missing_required_sources(
    case: &Value,
    matched_hit: Option<&Value>,
    hits: &[Value],
) -> Vec<String> {
    let required = string_vec(case, "required_sources");
    if required.is_empty() {
        return Vec::new();
    }
    let observed = if let Some(hit) = matched_hit {
        hit_sources(hit)
    } else {
        hits.iter().flat_map(hit_sources).collect::<Vec<_>>()
    };
    required
        .into_iter()
        .filter(|source| !observed.contains(source))
        .collect()
}

fn hit_sources(hit: &Value) -> Vec<String> {
    hit.get("retriever_sources")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn missing_required_backends(case: &Value, payload: &Value) -> Vec<String> {
    let required = case
        .get("required_backend_states")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let states = payload
        .get("backend_statuses")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|status| {
                    Some((
                        status.get("source")?.as_str()?.to_owned(),
                        status.get("state")?.as_str()?.to_owned(),
                    ))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    required
        .into_iter()
        .filter_map(|(source, allowed)| {
            let allowed = allowed
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            let current = states.get(&source).map(String::as_str);
            (!current.is_some_and(|state| allowed.contains(&state)))
                .then(|| format!("{}:{}", source, current.unwrap_or("missing")))
        })
        .collect()
}

include!("evaluator_judge.rs");

fn parse_json_output(stdout: &str) -> Value {
    parse_json_output_value(stdout).unwrap_or(Value::Null)
}

fn parse_json_output_value(stdout: &str) -> Option<Value> {
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_str(line).ok())
}

fn parse_json_case_output(
    case: &Value,
    repository: &str,
    objective: &str,
    result: &CommandResult,
) -> Result<Value, Box<CaseObservation>> {
    parse_json_output_value(&result.stdout).ok_or_else(|| Box::new(CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repository.to_owned(),
        passed: false,
        rank: None,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: 0,
        message: "invalid JSON output from --format json command".to_owned(),
        objective: objective.to_owned(),
        score_override: Some(0.0),
    }))
}

fn push_latency_metrics(
    metrics: &mut Vec<MetricObservation>,
    config: &Value,
    prefix: &str,
    durations: &[u64],
) {
    if durations.is_empty() {
        return;
    }
    metrics.push(MetricObservation {
        name: format!("{prefix}_p50_ms"),
        value: percentile(durations, 50) as f64,
        budget: budget(config, "query_p50_budget_ms"),
        lower_is_better: true,
        key: false,
    });
    metrics.push(MetricObservation {
        name: format!("{prefix}_p95_ms"),
        value: percentile(durations, 95) as f64,
        budget: budget(config, "query_p95_budget_ms"),
        lower_is_better: true,
        key: true,
    });
}

fn percentile(values: &[u64], percentile_value: u64) -> u64 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let index = ((ordered.len() - 1) as u64 * percentile_value / 100) as usize;
    ordered[index]
}

fn budget(value: &Value, name: &str) -> Option<f64> {
    value
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0)
}

fn normalized_env(env: &BTreeMap<String, String>, name: &str, default: &str) -> String {
    env.get(name)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn repo_report(
    repo_name: &str,
    scope: String,
    commands: Vec<CommandResult>,
    cases: Vec<CaseObservation>,
    metrics: Vec<MetricObservation>,
    index_summary: Value,
) -> RepoReport {
    let passed_commands = commands.iter().filter(|command| command.passed()).count();
    let passed_cases = cases.iter().filter(|case| case.passed).count();
    let command_duration_ms = commands
        .iter()
        .map(|command| command.duration_ms)
        .sum::<u64>();
    eprintln!(
        "[self-iterate] report done name={} commands={}/{} cases={}/{} metrics={} command_duration_ms={}",
        repo_name,
        passed_commands,
        commands.len(),
        passed_cases,
        cases.len(),
        metrics.len(),
        command_duration_ms
    );
    RepoReport {
        repository: repo_name.to_owned(),
        scope,
        commands,
        gates: Vec::new(),
        cases,
        metrics,
        index_summary,
    }
}

fn serializable_repo_report(report: &RepoReport) -> Value {
    serde_json::json!({
        "repository": report.repository,
        "scope": report.scope,
        "commands": report.commands.iter().map(CommandResult::serializable).collect::<Vec<_>>(),
        "gates": report.gates,
        "cases": report.cases,
        "metrics": report.metrics,
        "index_summary": report.index_summary.get("summary").cloned().unwrap_or_else(|| report.index_summary.clone()),
    })
}

#[cfg(test)]
include!("evaluator_tests.rs");
