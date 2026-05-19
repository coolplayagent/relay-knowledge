fn quality_gate_commands(profile: &str) -> Vec<(&'static str, Vec<String>, u64)> {
    if profile == "smoke" {
        return vec![(
            "cargo_fmt_check",
            vec!["cargo", "fmt", "--all", "--", "--check"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            120,
        )];
    }
    vec![
        (
            "cargo_build_release",
            vec!["cargo", "build", "--release"],
            1200,
        ),
        (
            "cargo_fmt_check",
            vec!["cargo", "fmt", "--all", "--", "--check"],
            120,
        ),
        (
            "cargo_clippy",
            vec![
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
        (
            "cargo_test",
            vec!["cargo", "test", "--all-targets", "--all-features"],
            1200,
        ),
    ]
    .into_iter()
    .map(|(name, command, timeout)| {
        (
            name,
            command.into_iter().map(ToOwned::to_owned).collect(),
            timeout,
        )
    })
    .collect()
}

fn quality_budget_ms(name: &str) -> Option<f64> {
    match name {
        "cargo_build_release" => Some(180_000.0),
        "cargo_fmt_check" => Some(20_000.0),
        "cargo_clippy" => Some(180_000.0),
        "cargo_test" => Some(240_000.0),
        _ => None,
    }
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
    let payload = parse_json_output(&result.stdout);
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
    let payload = parse_json_output(&result.stdout);
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let max_rank = number_or(case, "max_rank", 1) as usize;
    let assessment = assess_ranked_hits(case, hits, expected, forbidden);
    let mut passed = assessment.failures.is_empty();
    let mut rank = assessment.rank;
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        passed = hits.is_empty();
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
            "results={} rank={rank:?} {}",
            hits.len(),
            assessment.details
        ),
        objective,
        score_override: Some(if passed { assessment.score } else { 0.0 }),
    }
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
    let fixture_env = background_file_env(&runtime.env, &root, number_or(case, "scan_interval_ms", 250));
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
        + std::time::Duration::from_secs(runtime.timeout.min(number_or(case, "timeout_seconds", 8)));
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
        std::thread::sleep(std::time::Duration::from_millis(number_or(
            case,
            "poll_interval_ms",
            200,
        )));
    }
    let _ = service.kill();
    let _ = service.wait();
    let duration_ms = started.elapsed().as_millis() as u64;
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
    let payload = parse_json_output(&result.stdout);
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

#[derive(Debug, Clone)]
struct JudgeSettings {
    enabled: bool,
    missing: Vec<String>,
    command: String,
    timeout_seconds: u64,
}

fn judge_settings(env: &BTreeMap<String, String>) -> JudgeSettings {
    let backend = env
        .get("RELAY_KNOWLEDGE_JUDGE_BACKEND")
        .map(|value| value.trim().to_ascii_lowercase().replace('-', "_"))
        .unwrap_or_default();
    let timeout_seconds = env
        .get("RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120)
        .max(1);
    if ["none", "off", "disabled", "skip", "false"].contains(&backend.as_str()) {
        return JudgeSettings {
            enabled: false,
            missing: Vec::new(),
            command: String::new(),
            timeout_seconds,
        };
    }
    let command = ["RELAY_KNOWLEDGE_JUDGE_COMMAND", "RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND", "RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND"]
        .iter()
        .find_map(|name| env.get(*name).filter(|value| !value.trim().is_empty()).cloned())
        .unwrap_or_else(|| {
            "opencode run \"Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests.\" --file {prompt_file}".to_owned()
        });
    JudgeSettings {
        enabled: true,
        missing: Vec::new(),
        command,
        timeout_seconds,
    }
}

fn settings_summary(settings: &JudgeSettings) -> Value {
    serde_json::json!({
        "backend": "cli",
        "enabled": settings.enabled,
        "configured": settings.enabled && settings.missing.is_empty(),
        "missing": settings.missing,
        "timeout_seconds": settings.timeout_seconds,
        "cli_command_configured": !settings.command.is_empty(),
    })
}

struct JudgePromptInput<'a> {
    workspace: &'a Path,
    suite: &'a Value,
    generated_diff: bool,
    candidate_diff: &'a str,
    gates: &'a [GateObservation],
    cases: &'a [CaseObservation],
    metrics: &'a [MetricObservation],
    repo_reports: &'a [RepoReport],
}

fn build_judge_prompt(input: JudgePromptInput<'_>) -> String {
    let max_doc_chars = number_or(input.suite, "max_doc_chars", 3000) as usize;
    let max_diff_chars = number_or(input.suite, "max_diff_chars", 30000) as usize;
    let mut diff = input.candidate_diff.trim().to_owned();
    if diff.len() > max_diff_chars {
        diff.truncate(max_diff_chars);
        diff.push_str("\n...diff truncated...");
    }
    format!(
        "You are the relay-knowledge research judge.\nReturn only one strict JSON object with passed, confidence, overall_score, scores, summary, evidence, risks, recommended_cases.\n\nDeterministic summary:\n{}\n\nCandidate diff:\n```diff\n{}\n```\n\nReference document excerpts:\n{}",
        deterministic_summary(
            input.gates,
            input.cases,
            input.metrics,
            input.repo_reports,
            input.generated_diff
        ),
        diff,
        document_excerpts(input.workspace, input.suite, max_doc_chars)
    )
}

fn deterministic_summary(
    gates: &[GateObservation],
    cases: &[CaseObservation],
    metrics: &[MetricObservation],
    repo_reports: &[RepoReport],
    generated_diff: bool,
) -> String {
    serde_json::json!({
        "generated_diff": generated_diff,
        "gate_count": gates.len(),
        "failed_gates": gates.iter().filter(|gate| !gate.passed).map(|gate| &gate.name).collect::<Vec<_>>(),
        "case_count": cases.len(),
        "failed_cases": cases.iter().filter(|case| !case.passed).take(16).map(|case| &case.case_id).collect::<Vec<_>>(),
        "metrics": metrics.iter().take(16).map(|metric| format!("{}={}", metric.name, metric.value)).collect::<Vec<_>>(),
        "report_sections": repo_reports.iter().map(|report| &report.repository).collect::<Vec<_>>(),
    })
    .to_string()
}

fn document_excerpts(workspace: &Path, suite: &Value, max_doc_chars: usize) -> String {
    let default_docs = vec![
        "docs/zh/02-capabilities/15-evaluation-and-quality-gates.md".to_owned(),
        "docs/zh/03-architecture-specs/02-engineering-hard-constraints.md".to_owned(),
        "docs/zh/04-research/08-competitive-performance-research-2026.md".to_owned(),
    ];
    let docs = if array_field(suite, "documents").is_empty() {
        default_docs
    } else {
        string_vec(suite, "documents")
    };
    docs.into_iter()
        .map(|relative| {
            let text = fs::read_to_string(workspace.join(&relative))
                .unwrap_or_else(|_| "(missing)".to_owned());
            let excerpt = text.chars().take(max_doc_chars).collect::<String>();
            format!("## {relative}\n{excerpt}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn judge_cli_command(
    template: &str,
    workspace: &Path,
    prompt_file: &Path,
    prompt: &str,
) -> Result<(Vec<String>, Option<String>), String> {
    let parts = shell_split(template)?;
    let mut used_prompt = false;
    let mut command = Vec::new();
    for part in parts {
        let mut value = part.replace("{workspace}", &workspace.display().to_string());
        if value.contains("{prompt_file}") {
            used_prompt = true;
            value = value.replace("{prompt_file}", &prompt_file.display().to_string());
        }
        if value.contains("{prompt}") {
            used_prompt = true;
            value = value.replace("{prompt}", prompt);
        }
        command.push(value);
    }
    if command.is_empty() {
        return Err("empty judge command".to_owned());
    }
    Ok((command, (!used_prompt).then(|| prompt.to_owned())))
}

fn judge_outcome(text: &str, suite: &Value) -> (bool, bool, f64, String, Value) {
    let payload = extract_json_object(text)
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .unwrap_or_else(|| serde_json::json!({"passed": false, "overall_score": 0.0, "summary": "invalid judge JSON"}));
    let score = payload
        .get("overall_score")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let confidence = payload
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let anti_fixture = payload
        .get("scores")
        .and_then(|scores| scores.get("anti_fixture_special_casing"))
        .and_then(Value::as_f64)
        .unwrap_or(score);
    let passed = payload
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && score
            >= suite
                .get("min_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.75)
        && confidence
            >= suite
                .get("min_confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.6)
        && anti_fixture
            >= suite
                .get("min_anti_fixture_special_casing")
                .and_then(Value::as_f64)
                .unwrap_or(0.75);
    let message = payload
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("judge completed")
        .to_owned();
    (passed, passed, score, message, payload)
}

fn shell_split(value: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && (ch == '"' || ch == '\'') {
            quote = Some(ch);
        } else if quote.is_none() && ch.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if quote.is_some() {
        return Err("unterminated quote in command".to_owned());
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end >= start).then(|| text[start..=end].to_owned())
}

fn parse_json_output(stdout: &str) -> Value {
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| serde_json::from_str(line).ok())
        .unwrap_or(Value::Null)
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
mod tests {
    use super::*;

    #[test]
    fn shell_split_keeps_quoted_argument() {
        assert_eq!(
            shell_split("tool run \"hello world\" --file {prompt_file}").expect("split"),
            vec!["tool", "run", "hello world", "--file", "{prompt_file}"]
        );
    }

    #[test]
    fn percentile_selects_expected_rank() {
        assert_eq!(percentile(&[10, 20, 30, 40], 50), 20);
        assert_eq!(percentile(&[10, 20, 30, 40], 95), 30);
    }
}
