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
                linux_glibc_compatibility_policy_gate(),
            ]),
            QualityGateStage::Parallel(vec![quality_gate(
                "cargo_build_debug",
                ["cargo", "build", "--bin", "relay-knowledge"],
                600,
            )]),
            QualityGateStage::Parallel(vec![
                quality_gate(
                    "self_iteration_cargo_check",
                    [
                        "cargo",
                        "check",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                    ],
                    180,
                ),
                quality_gate(
                    "code_index_recovery_cases",
                    ["cargo", "test", "--all-targets", "code_index_task_"],
                    300,
                ),
            ]),
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
            linux_glibc_compatibility_policy_gate(),
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

fn linux_glibc_compatibility_policy_gate() -> QualityGate {
    quality_gate(
        "linux_glibc_compatibility_policy",
        [
            "python3",
            "tools/release/check_linux_glibc_compat.py",
            "--self-test",
            "--verify-workflow",
            ".github/workflows/release.yml",
        ],
        60,
    )
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
        "code_index_recovery_cases" => Some(60_000.0),
        "self_iteration_cargo_check" => Some(30_000.0),
        "cargo_build_release" => Some(180_000.0),
        "self_iteration_cargo_build_release" => Some(60_000.0),
        "cargo_fmt_check" => Some(20_000.0),
        "self_iteration_cargo_fmt_check" => Some(20_000.0),
        "linux_glibc_compatibility_policy" => Some(10_000.0),
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

#[derive(Debug, Clone)]
struct WorkloadSelection {
    categories: Option<CategorySet>,
}

impl WorkloadSelection {
    fn new(config: &Config) -> Self {
        Self {
            categories: config.categories.clone(),
        }
    }

    fn focused(&self) -> bool {
        self.categories.is_some()
    }

    fn contains(&self, category: EvaluationCategory) -> bool {
        self.categories
            .as_ref()
            .is_some_and(|categories| categories.contains(category))
    }

    fn selected_categories_report(&self) -> Value {
        self.categories
            .as_ref()
            .map(|categories| {
                Value::Array(
                    categories
                        .labels()
                        .into_iter()
                        .map(|label| Value::String(label.to_owned()))
                        .collect(),
                )
            })
            .unwrap_or(Value::Null)
    }

    fn runs_repository_workload(&self, profile: &str) -> bool {
        profile != "smoke"
    }

    fn runs_repository_sets(&self, profile: &str) -> bool {
        if profile == "smoke" {
            return false;
        }
        self.focused() || profile_runs_repository_sets(profile)
    }

    fn runs_file_fixtures(&self, profile: &str) -> bool {
        self.contains(EvaluationCategory::FileFixtures)
            || self.contains(EvaluationCategory::Performance)
            || (!self.focused() && profile_runs_slow_suites(profile))
    }

    fn runs_semantic_vector(&self, profile: &str) -> bool {
        if profile == "smoke" {
            return false;
        }
        self.focused() || profile == "fast" || profile_runs_slow_suites(profile)
    }

    fn runs_research_judge(&self, profile: &str) -> bool {
        self.contains(EvaluationCategory::ResearchJudge)
            || (!self.focused() && profile_runs_slow_suites(profile))
    }

    fn skipped_suites(&self, profile: &str) -> Vec<&'static str> {
        let mut skipped = Vec::new();
        if !self.runs_repository_workload(profile) {
            skipped.push("repository_evaluation");
        }
        if !self.runs_repository_sets(profile) {
            skipped.push("repository_sets");
        }
        if !self.runs_file_fixtures(profile) {
            skipped.push("file_fixtures");
        }
        if !self.runs_semantic_vector(profile) {
            skipped.push("semantic_vector");
        }
        if !self.runs_research_judge(profile) {
            skipped.push("research_judge");
        }
        skipped
    }
}

fn repository_in_profile(profile: &str, repo_name: &str, repo_config: &Value) -> bool {
    if repo_config.get("profile").and_then(Value::as_str) == Some("exhaustive")
        && profile != "exhaustive"
    {
        return false;
    }
    profile != "fast" || fast_repository_names().iter().any(|name| name == repo_name)
}

fn select_repository_cases_for_profile(
    profile: &str,
    categories: Option<&CategorySet>,
    cases: Vec<Value>,
) -> Vec<Value> {
    let filtered = if let Some(categories) = categories {
        cases
            .into_iter()
            .filter(|case| focused_repository_case(categories, case))
            .collect()
    } else {
        cases
    };
    limit_cases_for_profile(profile, filtered)
}

fn semantic_vector_suite_for_selection(
    suite: &Value,
    profile: &str,
    categories: Option<&CategorySet>,
) -> Value {
    let all_cases = array_field(suite, "query_cases").to_vec();
    let selected_cases = if categories
        .map(|items| {
            items.contains(EvaluationCategory::SemanticVector)
                || items.contains(EvaluationCategory::Performance)
        })
        .unwrap_or_else(|| profile_runs_slow_suites(profile))
    {
        all_cases
    } else {
        semantic_vector_guardrail_cases(all_cases)
    };
    let mut scoped = suite.clone();
    if let Some(object) = scoped.as_object_mut() {
        object.insert("query_cases".to_owned(), Value::Array(selected_cases));
    }
    scoped
}

fn semantic_vector_guardrail_cases(cases: Vec<Value>) -> Vec<Value> {
    let guardrails = cases
        .iter()
        .filter(|case| is_guardrail_case(case))
        .cloned()
        .collect::<Vec<_>>();
    if guardrails.is_empty() {
        cases.into_iter().take(1).collect()
    } else {
        guardrails
    }
}

fn focused_repository_case(categories: &CategorySet, case: &Value) -> bool {
    is_guardrail_case(case)
        || categories.contains(EvaluationCategory::Performance)
        || (categories.contains(EvaluationCategory::Foundational)
            && repository_case_objective(case) == "foundational_capability")
        || (categories.contains(EvaluationCategory::Competitive)
            && repository_case_objective(case) == "competitive_capability")
}

fn limit_cases_for_profile(profile: &str, cases: Vec<Value>) -> Vec<Value> {
    let Some(limit) = fast_case_limit(profile) else {
        return cases;
    };
    limit_preserving_guardrails(cases, limit)
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
    limit_preserving_guardrails(cases, fast_repository_set_case_limit())
}

fn limit_preserving_guardrails(cases: Vec<Value>, limit: usize) -> Vec<Value> {
    let mut selected = Vec::new();
    let mut selected_ids = BTreeSet::new();
    for case in cases.iter().filter(|case| is_guardrail_case(case)) {
        if selected_ids.insert(case_identity(case)) {
            selected.push(case.clone());
        }
    }
    for case in cases
        .into_iter()
        .filter(|case| !is_guardrail_case(case))
        .take(limit)
    {
        if selected_ids.insert(case_identity(&case)) {
            selected.push(case);
        }
    }
    selected
}

fn is_guardrail_case(case: &Value) -> bool {
    case
        .get("guardrail")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn case_identity(case: &Value) -> String {
    string_or(case, "id", "case").to_owned()
}

fn guardrail_gate_from_case(
    observation: &CaseObservation,
    duration_ms: u64,
) -> Option<GateObservation> {
    observation.guardrail.then(|| GateObservation {
        name: format!("guardrail_case_{}", observation.case_id),
        passed: observation.passed,
        duration_ms,
        message: observation.message.clone(),
    })
}

fn fast_case_limit(profile: &str) -> Option<usize> {
    (profile == "fast").then(|| {
        std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_CASE_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(8)
        })
}

fn fast_repository_set_case_limit() -> usize {
    std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_FAST_REPO_SET_CASE_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(2)
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
                "grep_budget_fixture".to_owned(),
                "c_syntax_fixture".to_owned(),
                "cpp_syntax_fixture".to_owned(),
                "cross_language_syntax_fixture".to_owned(),
                "typescript_syntax_fixture".to_owned(),
                "nonstandard_layout_fixture".to_owned(),
                "project_alias_fixture".to_owned(),
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

fn evaluate_registration_cases(
    runtime: &EvalRuntime,
    run_home: &Path,
    repository_configs: &BTreeMap<String, Value>,
    cases_config: &Value,
    profile: &str,
    categories: Option<&CategorySet>,
) -> Result<RegistrationCaseReport, String> {
    let selected_cases = select_registration_cases_for_profile(
        profile,
        categories,
        array_field(cases_config, "registration_cases").to_vec(),
    );
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut gates = Vec::new();
    if selected_cases.is_empty() {
        return Ok(RegistrationCaseReport {
            commands,
            cases,
            gates,
        });
    }
    eprintln!(
        "[self-iterate] registration guardrail workload start cases={}",
        selected_cases.len()
    );
    for case in selected_cases {
        let repo_name = string_or(&case, "repository", "");
        let Some(repo_config) = repository_configs.get(repo_name) else {
            let command = CommandResult {
                name: format!(
                    "registration_case_{}_repository_config",
                    string_or(&case, "id", "case")
                ),
                command: vec!["validate".to_owned(), "repository_config".to_owned()],
                exit_code: 1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: format!("registration case repository is not configured: {repo_name}"),
            };
            let observation = score_registration_case(repo_name, &case, &command);
            if let Some(gate) = guardrail_gate_from_case(&observation, command.duration_ms) {
                gates.push(gate);
            }
            commands.push(command);
            cases.push(observation);
            continue;
        };
        let (path, setup_commands) =
            prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
        let setup_passed = setup_commands.iter().all(CommandResult::passed);
        commands.extend(setup_commands);
        if !setup_passed {
            let failed = CommandResult {
                name: format!(
                    "registration_case_{}_setup",
                    string_or(&case, "id", "case")
                ),
                command: vec!["prepare".to_owned(), repo_name.to_owned()],
                exit_code: 1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: "registration case fixture setup failed".to_owned(),
            };
            let observation = score_registration_case(repo_name, &case, &failed);
            if let Some(gate) = guardrail_gate_from_case(&observation, failed.duration_ms) {
                gates.push(gate);
            }
            commands.push(failed);
            cases.push(observation);
            continue;
        }
        let alias = registration_case_alias(repo_config, &case);
        let register = run_writer_limited(
            runtime,
            CommandSpec::new(
                format!("registration_case_{}", string_or(&case, "id", "case")),
                registration_case_command(&runtime.binary, &path, &alias, &case),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        let observation = score_registration_case(repo_name, &case, &register);
        if let Some(gate) = guardrail_gate_from_case(&observation, register.duration_ms) {
            gates.push(gate);
        }
        commands.push(register);
        cases.push(observation);
    }

    Ok(RegistrationCaseReport {
        commands,
        cases,
        gates,
    })
}

fn select_registration_cases_for_profile(
    profile: &str,
    categories: Option<&CategorySet>,
    cases: Vec<Value>,
) -> Vec<Value> {
    cases
        .into_iter()
        .filter(|case| string_field(case, "profile") != Some("exhaustive") || profile == "exhaustive")
        .filter(|case| {
            categories
                .map(|items| focused_repository_case(items, case))
                .unwrap_or(true)
        })
        .collect()
}

fn registration_case_alias(repo_config: &Value, case: &Value) -> String {
    string_field(case, "alias").map(ToOwned::to_owned).unwrap_or_else(|| {
        format!(
            "{}-{}",
            string_or(repo_config, "alias", "registration-case"),
            string_or(case, "id", "case")
        )
    })
}

fn registration_case_command(binary: &Path, path: &Path, alias: &str, case: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "register".to_owned(),
        path.display().to_string(),
        "--alias".to_owned(),
        alias.to_owned(),
    ];
    for path_filter in string_vec(case, "path_filters") {
        command.extend(["--path".to_owned(), path_filter]);
    }
    for language in string_vec(case, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn register_command(binary: &Path, path: &Path, alias: Option<&str>) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "register".to_owned(),
        path.display().to_string(),
    ];
    if let Some(alias) = alias {
        command.extend(["--alias".to_owned(), alias.to_owned()]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
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

fn score_registration_case(
    repo_name: &str,
    case: &Value,
    result: &CommandResult,
) -> CaseObservation {
    let objective = repository_case_objective(case);
    let expect_failure = case
        .get("expect_failure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let output = format!("{}\n{}", result.stdout, result.stderr);
    let required_output = string_field(case, "output_contains")
        .or_else(|| string_field(case, "stderr_contains"));
    let output_matches = required_output.is_none_or(|expected| output.contains(expected));
    let passed = if expect_failure {
        !result.passed() && output_matches
    } else {
        result.passed() && output_matches
    };
    let message = if passed {
        format!(
            "exit_code={} expected_failure={} output_contains={:?}",
            result.exit_code, expect_failure, required_output
        )
    } else if let Some(expected) = required_output {
        format!(
            "exit_code={} expected_failure={} missing_output={expected:?} tail={}",
            result.exit_code,
            expect_failure,
            result.gate_message()
        )
    } else {
        format!(
            "exit_code={} expected_failure={} tail={}",
            result.exit_code,
            expect_failure,
            result.gate_message()
        )
    };

    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank: passed.then_some(0),
        max_rank: 1,
        false_positive_count: 0,
        message,
        objective,
        score_override: Some(if passed { 1.0 } else { 0.0 }),
    }
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
    let payload_failures = payload_constraint_failures(case, &payload, hits.len());
    let mut assessment = assess_ranked_hits(case, hits, expected, forbidden);
    assessment.failures.extend(payload_failures.clone());
    if !payload_failures.is_empty() {
        assessment.details = format!(
            "{} payload_failures={}",
            assessment.details,
            payload_failures.join("; ")
        );
    }
    let mut rank = assessment.rank;
    let mut passed = assessment.failures.is_empty();
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut failures = if hits.is_empty() {
            Vec::new()
        } else {
            vec![format!("expected_empty_results={}", hits.len())]
        };
        failures.extend(payload_failures);
        passed = failures.is_empty();
        rank = passed.then_some(0);
        assessment = RankedAssessment {
            rank,
            false_positive_count: 0,
            score: if passed { 1.0 } else { 0.0 },
            details: if failures.is_empty() {
                "expect_empty".to_owned()
            } else {
                format!("expect_empty failures={}", failures.join("; "))
            },
            failures,
        };
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
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
        guardrail: is_guardrail_case(case),
        rank: None,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: 0,
        message: result.gate_message(),
        objective: objective.to_owned(),
        score_override: None,
    }
}

fn prepare_repository_path(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
) -> Result<(PathBuf, Vec<CommandResult>), String> {
    let Some(fixture) = string_field(repo_config, "generated_fixture") else {
        return Ok((PathBuf::from(string_or(repo_config, "path", "")), Vec::new()));
    };
    let root = generated_repository_root(run_home, repo_name)?;
    create_generated_repository_files(&root, fixture)?;
    Ok((
        root.clone(),
        commit_generated_repository(runtime, repo_name, &root),
    ))
}

fn generated_repository_root(run_home: &Path, repo_name: &str) -> Result<PathBuf, String> {
    if repo_name.is_empty()
        || !repo_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "generated repository name must be a safe path component: {repo_name:?}"
        ));
    }
    Ok(run_home.join("generated-repositories").join(repo_name))
}

fn create_generated_repository_files(root: &Path, fixture: &str) -> Result<(), String> {
    if root.exists() {
        fs::remove_dir_all(root)
            .map_err(|error| format!("failed to remove {}: {error}", root.display()))?;
    }
    fs::create_dir_all(root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    if fixture == "grep_budget_v1" {
        return write_grep_budget_fixture(root);
    }
    for (path, content) in generated_repository_files(fixture)? {
        write_fixture_file(&root.join(path), content)?;
    }
    Ok(())
}

fn generated_repository_files(fixture: &str) -> Result<Vec<(&'static str, &'static str)>, String> {
    match fixture {
        "c_syntax_v1" => Ok(vec![
            ("include/driver_ops.h", C_DRIVER_OPS_H),
            ("include/macros.h", C_MACROS_H),
            ("src/driver_ops.c", C_DRIVER_OPS_C),
            ("src/dispatch.c", C_DISPATCH_C),
            ("src/generated_table.c", C_GENERATED_TABLE_C),
            ("src/http_macro_module.c", C_HTTP_MACRO_MODULE_C),
            ("src/nginx_external_module.c", C_NGINX_EXTERNAL_MODULE_C),
            ("tests/fake_driver.c", C_FAKE_DRIVER_C),
        ]),
        "cpp_syntax_v1" => Ok(vec![
            ("include/store/cache.hpp", CPP_CACHE_HPP),
            ("include/store/exported_module.hpp", CPP_EXPORTED_MODULE_HPP),
            ("include/store/pipeline.hpp", CPP_PIPELINE_HPP),
            ("src/cache.cpp", CPP_CACHE_CPP),
            ("src/pipeline.cpp", CPP_PIPELINE_CPP),
            ("tests/fake_cache.cpp", CPP_FAKE_CACHE_CPP),
        ]),
        "cross_language_syntax_v1" => Ok(vec![
            (
                ".relay-knowledge-fixture-version",
                "cross_language_syntax_v1\n",
            ),
            ("include/rk_bridge.h", CROSS_LANGUAGE_BRIDGE_H),
            ("src/c_entry.c", CROSS_LANGUAGE_C_ENTRY),
            ("src/cpp_bridge.cpp", CROSS_LANGUAGE_CPP_BRIDGE),
            ("bridge/go_bridge.go", CROSS_LANGUAGE_GO_BRIDGE),
            ("crates/rust_bridge/src/lib.rs", CROSS_LANGUAGE_RUST_BRIDGE),
            ("tests/fake_bridge.c", CROSS_LANGUAGE_FAKE_BRIDGE),
        ]),
        "project_alias_v1" => Ok(vec![("src/lib.rs", PROJECT_ALIAS_LIB_RS)]),
        "python_syntax_v2" => Ok(vec![
            ("docs/operations.md", PYTHON_OPERATIONS_MD),
            ("syntax_service/__init__.py", PYTHON_INIT),
            ("syntax_service/decorators.py", PYTHON_DECORATORS),
            ("syntax_service/errors.py", PYTHON_ERRORS),
            ("syntax_service/service.py", PYTHON_SERVICE),
            ("tests/fake_service.py", PYTHON_FAKE_SERVICE),
        ]),
        "javascript_syntax_v2" => Ok(vec![
            ("src/runtime.js", JAVASCRIPT_RUNTIME),
            ("src/registry.js", JAVASCRIPT_REGISTRY),
            ("src/index.js", JAVASCRIPT_INDEX),
            ("tests/fakeRuntime.js", JAVASCRIPT_FAKE_RUNTIME),
        ]),
        "typescript_syntax_v2" => Ok(vec![
            ("src/protocol.ts", TYPESCRIPT_PROTOCOL),
            ("src/provider.ts", TYPESCRIPT_PROVIDER),
            ("src/component.tsx", TYPESCRIPT_COMPONENT),
            ("src/index.ts", TYPESCRIPT_INDEX),
            ("tests/fakeProvider.ts", TYPESCRIPT_FAKE_PROVIDER),
        ]),
        "go_syntax_v2" => Ok(vec![
            ("go.mod", GO_MOD),
            ("processor/worker.go", GO_WORKER),
            ("processor/pipeline.go", GO_PIPELINE),
            ("tests/fake_worker.go", GO_FAKE_WORKER),
        ]),
        "java_syntax_v2" => Ok(vec![
            ("src/main/java/example/ServiceContract.java", JAVA_SERVICE_CONTRACT),
            ("src/main/java/example/AnnotatedService.java", JAVA_ANNOTATED_SERVICE),
            ("src/main/java/example/ServiceFactory.java", JAVA_SERVICE_FACTORY),
            ("src/test/java/example/FakeService.java", JAVA_FAKE_SERVICE),
        ]),
        "rust_syntax_v2" => Ok(vec![
            ("src/lib.rs", RUST_LIB),
            ("src/service.rs", RUST_SERVICE),
            ("src/model.rs", RUST_MODEL),
            ("tests/fake_service.rs", RUST_FAKE_SERVICE),
        ]),
        "bash_syntax_v1" => Ok(vec![
            ("bin/install.sh", BASH_INSTALL),
            ("lib/runtime.sh", BASH_RUNTIME),
            ("tests/fake_runtime.sh", BASH_FAKE_RUNTIME),
        ]),
        "csharp_syntax_v2" => Ok(vec![
            ("src/Runtime/BufferPool.cs", CSHARP_BUFFER_POOL),
            ("src/Runtime/RuntimeService.cs", CSHARP_RUNTIME_SERVICE),
            ("tests/FakeRuntimeService.cs", CSHARP_FAKE_SERVICE),
        ]),
        "kotlin_syntax_v2" => Ok(vec![
            ("src/main/kotlin/example/Client.kt", KOTLIN_CLIENT),
            ("src/main/kotlin/example/Pipeline.kt", KOTLIN_PIPELINE),
            ("tests/FakeClient.kt", KOTLIN_FAKE_CLIENT),
        ]),
        "php_syntax_v2" => Ok(vec![
            ("src/App/Kernel.php", PHP_KERNEL),
            ("src/App/Contracts/Bootable.php", PHP_BOOTABLE),
            ("src/App/Providers/CacheProvider.php", PHP_CACHE_PROVIDER),
            ("tests/FakeKernel.php", PHP_FAKE_KERNEL),
        ]),
        "ruby_syntax_v2" => Ok(vec![
            ("lib/app/controller.rb", RUBY_CONTROLLER),
            ("lib/app/extensions.rb", RUBY_EXTENSIONS),
            ("lib/app/runtime.rb", RUBY_RUNTIME),
            ("tests/fake_controller.rb", RUBY_FAKE_CONTROLLER),
        ]),
        "scala_syntax_v2" => Ok(vec![
            ("src/main/scala/example/Pipeline.scala", SCALA_PIPELINE),
            ("src/main/scala/example/Runtime.scala", SCALA_RUNTIME),
            ("tests/FakePipeline.scala", SCALA_FAKE_PIPELINE),
        ]),
        "swift_syntax_v2" => Ok(vec![
            ("Sources/App/SessionClient.swift", SWIFT_SESSION_CLIENT),
            ("Sources/App/RequestPipeline.swift", SWIFT_REQUEST_PIPELINE),
            ("Tests/AppTests/FakeSessionClient.swift", SWIFT_FAKE_SESSION_CLIENT),
        ]),
        "nonstandard_layout_v1" => Ok(vec![
            (
                ".relay-knowledge-fixture-version",
                "nonstandard_layout_v1\n",
            ),
            (
                "external_deps/python_sdk/session_client.py",
                NONSTANDARD_PYTHON_SESSION_CLIENT,
            ),
            (
                "external_deps/ts_sdk/sessionClient.ts",
                NONSTANDARD_TYPESCRIPT_SESSION_CLIENT,
            ),
            (
                "plugins/example.com/nonstandard/session/client.go",
                NONSTANDARD_GO_SESSION_CLIENT,
            ),
            (
                "modules/java_sdk/src/main/java/example/ExternalJavaSessionClient.java",
                NONSTANDARD_JAVA_SESSION_CLIENT,
            ),
            (
                "external_deps/cpp_sdk/include/external_session_client.hpp",
                NONSTANDARD_CPP_SESSION_CLIENT_HPP,
            ),
            (
                "external_deps/cpp_sdk/session_client.cpp",
                NONSTANDARD_CPP_SESSION_CLIENT_CPP,
            ),
            (
                "Sources/SwiftSdk/ExternalSwiftSessionClient.swift",
                NONSTANDARD_SWIFT_SESSION_CLIENT,
            ),
            ("src/application.ts", NONSTANDARD_APPLICATION_TS),
            ("Cargo.toml", NONSTANDARD_CARGO_TOML),
            ("Cargo.lock", NONSTANDARD_CARGO_LOCK),
            ("web/package.json", NONSTANDARD_PACKAGE_JSON),
            ("web/package-lock.json", NONSTANDARD_PACKAGE_LOCK_JSON),
            ("go.mod", NONSTANDARD_GO_MOD),
            ("pyproject.toml", NONSTANDARD_PYPROJECT_TOML),
            ("modules/java_sdk/pom.xml", NONSTANDARD_POM_XML),
            ("modules/java_sdk/build.gradle.kts", NONSTANDARD_BUILD_GRADLE_KTS),
            ("external_deps/cpp_sdk/conanfile.txt", NONSTANDARD_CONANFILE_TXT),
            ("external_deps/cpp_sdk/conanfile.py", NONSTANDARD_CONANFILE_PY),
        ]),
        other => Err(format!("unknown generated repository fixture: {other}")),
    }
}

fn write_grep_budget_fixture(root: &Path) -> Result<(), String> {
    for index in 0..300 {
        write_fixture_file(
            &root.join("src").join(format!("noise_{index:03}.c")),
            &format!("int noise_{index:03}(void) {{ return {index}; }}\n"),
        )?;
    }
    write_fixture_file(
        &root.join("zzz").join("late_target.c"),
        r#"// RK_LATE_BUDGET_NOTE must remain reachable after broad candidate selection.
int late_budget_target(void)
{
    return 7;
}
"#,
    )
}

fn commit_generated_repository(
    runtime: &EvalRuntime,
    repo_name: &str,
    root: &Path,
) -> Vec<CommandResult> {
    let env = generated_git_env(&runtime.env);
    let commands = [
        vec!["git", "init", "-q"],
        vec!["git", "config", "user.email", "self-iteration@example.invalid"],
        vec!["git", "config", "user.name", "relay-knowledge self-iteration"],
        vec!["git", "add", "."],
        vec![
            "git",
            "commit",
            "--no-gpg-sign",
            "-q",
            "-m",
            "Generate relay-knowledge syntax fixture",
        ],
    ];
    commands
        .into_iter()
        .enumerate()
        .map(|(index, command)| {
            run_limited(
                &runtime.limiter,
                CommandSpec::new(
                    format!("{repo_name}_generated_fixture_git_{index}"),
                    command.into_iter().map(ToOwned::to_owned).collect(),
                    root,
                    Some(env.clone()),
                    runtime.timeout.min(30),
                ),
            )
        })
        .collect()
}

fn generated_git_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut scoped = env.clone();
    scoped.insert(
        "GIT_AUTHOR_DATE".to_owned(),
        "2026-05-20T00:00:00Z".to_owned(),
    );
    scoped.insert(
        "GIT_COMMITTER_DATE".to_owned(),
        "2026-05-20T00:00:00Z".to_owned(),
    );
    scoped
}

const C_DRIVER_OPS_H: &str = r#"#ifndef RK_DRIVER_OPS_H
#define RK_DRIVER_OPS_H

#include <stddef.h>

struct rk_device;

typedef int (*rk_open_fn)(struct rk_device *dev);
typedef int (*rk_read_fn)(struct rk_device *dev, char *buffer, size_t length);

struct rk_driver_ops {
    rk_open_fn open;
    rk_read_fn read;
    void (*close)(struct rk_device *dev);
};

// RK_HEADER_CONTRACT_NOTE keeps callback ownership searchable when parsers miss comments.
int rk_driver_open(struct rk_device *dev);
int rk_driver_read(struct rk_device *dev, char *buffer, size_t length);
void rk_driver_close(struct rk_device *dev);
int rk_dispatch_read(
    const struct rk_driver_ops *ops,
    struct rk_device *dev,
    char *buffer,
    size_t length);

#endif
"#;

const C_MACROS_H: &str = r#"#ifndef RK_MACROS_H
#define RK_MACROS_H

#define RK_STATUS_CLOSED 0
#define RK_STATUS_READY 1
#define RK_TRACE_VALUE(value) ((value) + 17)
#define RK_TOKEN_PASTE(left, right) left##right
#define RK_DECLARE_HANDLER(name) int name(struct rk_device *dev)

enum rk_stage {
    RK_STAGE_VALIDATE = 0,
    RK_STAGE_LOCK = 1,
    RK_STAGE_READ = 2,
};

#define RK_STAGE_ROW(name) [RK_STAGE_##name] = #name

#endif
"#;

const C_DRIVER_OPS_C: &str = r#"#include "driver_ops.h"
#include "macros.h"

struct rk_device {
    int fd;
    int state;
};

int rk_driver_open(struct rk_device *dev)
{
    dev->state = RK_STATUS_READY;
    return dev->state;
}

int rk_driver_read(struct rk_device *dev, char *buffer, size_t length)
{
    // RK_TRACE_NOTE documents fallback-only macro text.
    buffer[0] = (char)RK_TRACE_VALUE(dev->fd);
    return (int)length;
}

void rk_driver_close(struct rk_device *dev)
{
    dev->state = RK_STATUS_CLOSED;
}

const struct rk_driver_ops rk_default_ops = {
    .open = rk_driver_open,
    .read = rk_driver_read,
    .close = rk_driver_close,
};
"#;

const C_DISPATCH_C: &str = r#"#include "driver_ops.h"

static int rk_validate_device(struct rk_device *dev)
{
    return dev != 0;
}

static int rk_lock_device(struct rk_device *dev)
{
    return dev != 0;
}

static void rk_unlock_device(struct rk_device *dev)
{
    (void)dev;
}

typedef int (*rk_stage_fn)(struct rk_device *dev);

static rk_stage_fn rk_pipeline[] = {
    rk_validate_device,
    rk_lock_device,
};

int rk_dispatch_read(
    const struct rk_driver_ops *ops,
    struct rk_device *dev,
    char *buffer,
    size_t length)
{
    if (!rk_validate_device(dev)) {
        return -1;
    }
    if (ops->open(dev) < 0) {
        return -1;
    }
    if (rk_lock_device(dev) < 0) {
        return -1;
    }
    int result = ops->read(dev, buffer, length);
    rk_unlock_device(dev);
    return result;
}

int rk_run_pipeline(struct rk_device *dev)
{
    int total = 0;
    for (unsigned int index = 0; index < 2; ++index) {
        total += rk_pipeline[index](dev);
    }
    return total;
}

// RK_PIPELINE_NOTE records dispatch ordering for exact source-text fallback.
"#;

const C_GENERATED_TABLE_C: &str = r#"#include "driver_ops.h"
#include "macros.h"

struct rk_table_row {
    const char *name;
    rk_read_fn read;
};

static const char *rk_stage_names[] = {
    RK_STAGE_ROW(VALIDATE),
    RK_STAGE_ROW(LOCK),
    RK_STAGE_ROW(READ),
};

// RK_STAGE_TABLE_NOTE documents generated stage rows for grep fallback recall.
static const struct rk_table_row rk_rows[] = {
    [RK_STAGE_READ] = {
        .name = "read",
        .read = rk_driver_read,
    },
};

int rk_table_read(struct rk_device *dev, char *buffer, size_t length)
{
    (void)rk_stage_names;
    return rk_rows[RK_STAGE_READ].read(dev, buffer, length);
}
"#;

const C_HTTP_MACRO_MODULE_C: &str = r#"#include "driver_ops.h"
#include <openssl/ssl.h>

#define RK_HTTP_HANDLER(name) int name(struct rk_device *dev)
#define RK_HTTP_MODULE_ENTRY(name) { #name, name }

struct rk_http_module_entry {
    const char *name;
    int (*handler)(struct rk_device *dev);
};

RK_HTTP_HANDLER(rk_http_access_handler)
{
    return dev != 0;
}

static const struct rk_http_module_entry rk_http_modules[] = {
    RK_HTTP_MODULE_ENTRY(rk_http_access_handler),
};
"#;

const C_NGINX_EXTERNAL_MODULE_C: &str = r#"#include <ngx_config.h>
#include <ngx_core.h>
#include <ngx_http.h>

#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)

static ngx_int_t
ngx_http_demo_init(ngx_pool_t *pool)
{
    return ngx_array_init(pool);
}

KONG_ACCESS_PHASE(ngx_http_demo_access)
{
    return ngx_http_demo_init(request->pool);
}

static ngx_command_t ngx_http_demo_commands[] = {
    { ngx_string("demo"), NGX_HTTP_LOC_CONF, ngx_conf_set_str_slot, 0, 0, NULL },
    ngx_null_command
};

static ngx_http_module_t ngx_http_demo_module_ctx = {
    NULL,
    ngx_http_demo_init,
    NULL,
    NULL
};

ngx_module_t ngx_http_demo_module = {
    NGX_MODULE_V1,
    &ngx_http_demo_module_ctx,
    ngx_http_demo_commands,
    NGX_HTTP_MODULE,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NGX_MODULE_V1_PADDING
};
"#;

const C_FAKE_DRIVER_C: &str = r#"#include "driver_ops.h"

int rk_driver_read_fake(struct rk_device *dev, char *buffer, size_t length)
{
    (void)dev;
    (void)buffer;
    return (int)length;
}
"#;

const CPP_CACHE_HPP: &str = r#"#pragma once

#include <memory>
#include <string>
#include <vector>

namespace rk::store {

class Writer {
 public:
    virtual ~Writer() = default;
    virtual void Append(const std::string& key) = 0;
};

template <typename Key>
class Cache {
 public:
    using KeyList = std::vector<Key>;

    explicit Cache(std::unique_ptr<Writer> writer);
    void Insert(const Key& key);
    const Key& Lookup(const Key& key) const;

 private:
    std::unique_ptr<Writer> writer_;
    KeyList keys_;
};

class RecordingWriter final : public Writer {
 public:
    void Append(const std::string& key) override;
};

}  // namespace rk::store
"#;

const CPP_EXPORTED_MODULE_HPP: &str = r#"#pragma once

#include <boost/asio.hpp>
#include <string>

#define RK_STORE_API __attribute__((visibility("default")))

namespace rk::store {

class BaseModule {
 public:
    virtual ~BaseModule() = default;
};

RK_STORE_API class HttpModule final : public BaseModule {
 public:
    void Start(const std::string& route);
};

}  // namespace rk::store
"#;

const CPP_PIPELINE_HPP: &str = r#"#pragma once

#include "store/cache.hpp"

#include <memory>
#include <string>
#include <vector>

namespace rk::store {

struct PipelineEvent {
    std::string key;
};

class Pipeline {
 public:
    int operator()(const PipelineEvent& event) const;
};

std::unique_ptr<Cache<std::string>> BuildCache(std::unique_ptr<Writer> writer);
int RunPipeline(Cache<std::string>& cache, const std::vector<PipelineEvent>& events);

}  // namespace rk::store
"#;

const CPP_CACHE_CPP: &str = r#"#include "store/cache.hpp"

#include <utility>

namespace rk::store {

template <typename Key>
Cache<Key>::Cache(std::unique_ptr<Writer> writer) : writer_(std::move(writer)) {}

template <typename Key>
void Cache<Key>::Insert(const Key& key)
{
    keys_.push_back(key);
    writer_->Append(std::string(key));
}

template <typename Key>
const Key& Cache<Key>::Lookup(const Key& key) const
{
    for (const auto& candidate : keys_) {
        if (candidate == key) {
            return candidate;
        }
    }
    return keys_.front();
}

void RecordingWriter::Append(const std::string& key)
{
    (void)key;
}

template class Cache<std::string>;

}  // namespace rk::store
"#;

const CPP_PIPELINE_CPP: &str = r#"#include "store/pipeline.hpp"

#include <utility>

namespace rk::store {

namespace cache_alias = rk::store;

std::unique_ptr<Cache<std::string>> BuildCache(std::unique_ptr<Writer> writer)
{
    return std::make_unique<cache_alias::Cache<std::string>>(std::move(writer));
}

int Pipeline::operator()(const PipelineEvent& event) const
{
    return static_cast<int>(event.key.size());
}

int RunPipeline(Cache<std::string>& cache, const std::vector<PipelineEvent>& events)
{
    Pipeline pipeline;
    auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
        cache.Insert(event.key);
        return pipeline(event);
    };
    int total = 0;
    for (const auto& event : events) {
        total += append_event(event);
    }
    return total;
}

}  // namespace rk::store
"#;

const CPP_FAKE_CACHE_CPP: &str = r#"#include "store/cache.hpp"

namespace rk::store::test {

class FakeCache {
 public:
    void Insert(const std::string& key)
    {
        (void)key;
    }
};

}  // namespace rk::store::test
"#;

const CROSS_LANGUAGE_BRIDGE_H: &str = r#"#pragma once

#ifdef __cplusplus
extern "C" {
#endif

int rk_c_decode(const char *payload);
int rk_cpp_score(const char *payload);
int rk_c_entry_process(const char *payload);

#ifdef __cplusplus
}
#endif
"#;

const CROSS_LANGUAGE_C_ENTRY: &str = r#"#include "rk_bridge.h"

static int rk_c_weight(char value)
{
    return (int)value;
}

int rk_c_decode(const char *payload)
{
    if (payload == 0 || payload[0] == '\0') {
        return 0;
    }
    return rk_c_weight(payload[0]);
}

int rk_c_entry_process(const char *payload)
{
    int native = rk_c_decode(payload);
    int bridged = rk_cpp_score(payload);
    return native + bridged;
}
"#;

const CROSS_LANGUAGE_CPP_BRIDGE: &str = r#"#include "rk_bridge.h"

#include <string_view>

namespace rk::bridge {

class BridgeHelper {
 public:
    int Normalize(const char *payload) const
    {
        std::string_view view(payload == nullptr ? "" : payload);
        return static_cast<int>(view.size());
    }
};

}  // namespace rk::bridge

extern "C" int rk_cpp_score(const char *payload)
{
    rk::bridge::BridgeHelper helper;
    return helper.Normalize(payload) + rk_c_decode(payload);
}
"#;

const CROSS_LANGUAGE_GO_BRIDGE: &str = r#"package bridge

/*
#cgo CFLAGS: -I../include
#include <stdlib.h>
#include "rk_bridge.h"
*/
import "C"

import "unsafe"

func RunCgoBridge(payload string) int {
    cPayload := C.CString(payload)
    defer C.free(unsafe.Pointer(cPayload))
    return int(C.rk_c_decode(cPayload))
}
"#;

const CROSS_LANGUAGE_RUST_BRIDGE: &str = r#"use std::ffi::CString;
use std::os::raw::{c_char, c_int};

unsafe extern "C" {
    fn rk_c_decode(payload: *const c_char) -> c_int;
}

pub fn run_rust_bridge(payload: &str) -> i32 {
    let c_payload = CString::new(payload).expect("fixture payload should not contain nul bytes");
    unsafe { rk_c_decode(c_payload.as_ptr()) as i32 }
}
"#;

const CROSS_LANGUAGE_FAKE_BRIDGE: &str = r#"#include "rk_bridge.h"

int rk_cpp_score_fake(const char *payload)
{
    (void)payload;
    return 0;
}
"#;

const PROJECT_ALIAS_LIB_RS: &str = r#"
pub fn stable_project_entry() -> &'static str {
    "project-name-default-alias"
}

pub fn stable_project_session_entry() -> &'static str {
    stable_project_entry()
}
"#;

const PYTHON_OPERATIONS_MD: &str = r#"# Syntax service operations

The ServiceRunner class owns the async dispatch lifecycle for production workers.
The dispatch_event function normalizes payload text before writing event records.
"#;

const PYTHON_INIT: &str = r#""#;

const PYTHON_DECORATORS: &str = r#"
def traced_operation(name):
    def wrap(func):
        async def inner(*args, **kwargs):
            return await func(*args, **kwargs)
        inner.operation_name = name
        return inner
    return wrap
"#;

const PYTHON_ERRORS: &str = r#"
class ServiceError(RuntimeError):
    pass


class OverloadedServiceError(ServiceError):
    pass
"#;

const PYTHON_SERVICE: &str = r#"
from .decorators import traced_operation
from .errors import OverloadedServiceError, ServiceError


class AsyncResource:
    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def write_event(self, event):
        return event["payload"]


class ServiceRunner:
    def __init__(self, resource):
        self.resource = resource
        self.payload_filter = lambda value: value.strip()

    @traced_operation("dispatch")
    async def dispatch_event(self, event):
        async with self.resource as resource:
            payload = await resource.write_event(event)
            return self.normalize_payload(payload)

    def normalize_payload(self, payload):
        if payload == "overload":
            raise OverloadedServiceError("overload")
        return self.payload_filter(payload)


async def run_service(event):
    runner = ServiceRunner(AsyncResource())
    return await runner.dispatch_event(event)
"#;

const PYTHON_FAKE_SERVICE: &str = r#"
class ServiceRunner:
    def dispatch_event(self, event):
        return event
"#;

const JAVASCRIPT_RUNTIME: &str = r#"
import { createRegistry } from "./registry.js";

export class RuntimeController {
  constructor(registry = createRegistry()) {
    this.registry = registry;
  }

  async dispatchEvent(event) {
    const handler = this.registry.resolve(event.type);
    return handler(event.payload);
  }
}

export async function runRuntime(events) {
  const controller = new RuntimeController();
  return Promise.all(events.map((event) => controller.dispatchEvent(event)));
}
"#;

const JAVASCRIPT_REGISTRY: &str = r#"
export function createRegistry() {
  const handlers = new Map();
  const payloadPipeline = (payload) => normalizePayload(payload);
  handlers.set("write", payloadPipeline);
  return {
    resolve(type) {
      return handlers.get(type) ?? missingHandler;
    },
  };
}

export function normalizePayload(payload) {
  return String(payload).trim();
}

function missingHandler(payload) {
  throw new Error(`missing handler ${payload}`);
}
"#;

const JAVASCRIPT_INDEX: &str = r#"
export { RuntimeController, runRuntime } from "./runtime.js";
export { createRegistry, normalizePayload } from "./registry.js";
"#;

const JAVASCRIPT_FAKE_RUNTIME: &str = r#"
export class RuntimeController {
  dispatchEvent(event) {
    return event;
  }
}
"#;

const TYPESCRIPT_PROTOCOL: &str = r#"
export interface StreamTransport<TEvent> {
  send(event: TEvent): Promise<void>;
}

export type StreamEnvelope<TPayload> = {
  id: string;
  payload: TPayload;
};

export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;

export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();

export async function sendEnvelope<TPayload>(
  transport: StreamTransport<StreamEnvelope<TPayload>>,
  payload: TPayload,
): Promise<StreamEnvelope<TPayload>> {
  const envelope = { id: "syntax-envelope", payload };
  await transport.send(envelope);
  return envelope;
}
"#;

const TYPESCRIPT_PROVIDER: &str = r#"
import type { StreamEnvelope, StreamTransport } from "./protocol";
import { sendEnvelope } from "./protocol";
import { trimPayload } from "./protocol";

export class ProviderRuntime implements StreamTransport<StreamEnvelope<string>> {
  async send(event: StreamEnvelope<string>): Promise<void> {
    await import("./protocol");
    this.record(event.payload);
  }

  record(payload: string): string {
    return trimPayload(payload);
  }
}

export async function runProvider(payload: string): Promise<StreamEnvelope<string>> {
  const runtime = new ProviderRuntime();
  return sendEnvelope(runtime, payload);
}
"#;

const TYPESCRIPT_COMPONENT: &str = r#"
import React from "react";
import { runProvider } from "./provider";

export function ProviderPanel({ value }: { value: string }) {
  const [state, setState] = React.useState(value);
  React.useEffect(() => {
    runProvider(state).then((envelope) => setState(envelope.payload));
  }, [state]);
  return <section data-provider={state}>{state}</section>;
}
"#;

const TYPESCRIPT_INDEX: &str = r#"
export type { StreamEnvelope, StreamTransport } from "./protocol";
export { ProviderRuntime, runProvider } from "./provider";
export { ProviderPanel } from "./component";
"#;

const TYPESCRIPT_FAKE_PROVIDER: &str = r#"
export class ProviderRuntime {
  record(payload: string): string {
    return payload;
  }
}
"#;

const GO_MOD: &str = r#"module example.com/syntax

go 1.22
"#;

const GO_WORKER: &str = r#"
package processor

import (
    ctxalias "context"
    _ "embed"
    . "strings"
)

type EventProcessor interface {
    Process(ctx ctxalias.Context, event Event) error
}

type Event struct {
    Payload string
}

type Worker struct {
    processor EventProcessor
}

func NewWorker(processor EventProcessor) *Worker {
    return &Worker{processor: processor}
}

func (w *Worker) Run(ctx ctxalias.Context, events []Event) error {
    for _, event := range events {
        if err := w.processor.Process(ctx, event); err != nil {
            return err
        }
        _ = TrimSpace(event.Payload)
    }
    return nil
}
"#;

const GO_PIPELINE: &str = r#"
package processor

import "context"

type PipelineProcessor struct{}

func (PipelineProcessor) Process(ctx context.Context, event Event) error {
    done := make(chan struct{})
    notify := func(payload string) string {
        return payload
    }
    go func() {
        defer close(done)
        _ = notify(event.Payload)
    }()
    <-done
    return ctx.Err()
}

func RunPipeline(events []Event) error {
    worker := NewWorker(PipelineProcessor{})
    return worker.Run(context.Background(), events)
}
"#;

const GO_FAKE_WORKER: &str = r#"
package tests

type Worker struct{}

func (Worker) Run() {}
"#;

const JAVA_SERVICE_CONTRACT: &str = r#"
package example;

public interface ServiceContract<T> {
    default T normalize(T value) {
        return value;
    }

    T handle(T value);
}
"#;

const JAVA_ANNOTATED_SERVICE: &str = r#"
package example;

@Deprecated
public class AnnotatedService implements ServiceContract<String> {
    public AnnotatedService() {}

    @Override
    public String handle(String value) {
        return normalize(value).trim();
    }

    public static class Builder {
        public AnnotatedService build() {
            return new AnnotatedService();
        }
    }
}
"#;

const JAVA_SERVICE_FACTORY: &str = r#"
package example;

import example.AnnotatedService.Builder;
import java.util.function.Function;

public final class ServiceFactory {
    public ServiceContract<String> create() {
        Builder builder = new Builder();
        return builder.build();
    }

    public String dispatch(String value) {
        Function<String, String> transformer = ignored -> create().handle(value);
        return transformer.apply(value);
    }
}
"#;

const JAVA_FAKE_SERVICE: &str = r#"
package example;

class FakeService {
    String handle(String value) {
        return value;
    }
}
"#;

const RUST_LIB: &str = r#"
pub mod model;
pub mod service;

pub use service::{EventHandler, RuntimeService};
"#;

const RUST_MODEL: &str = r#"
pub enum RuntimeEvent {
    Start(String),
    Stop,
}
"#;

const RUST_SERVICE: &str = r#"
use crate::model::RuntimeEvent;

macro_rules! trace_event {
    ($event:expr) => {
        format!("trace::{:?}", $event)
    };
}

pub trait EventHandler {
    fn handle_event(&self, event: RuntimeEvent) -> String;
}

pub struct RuntimeService;

impl RuntimeService {
    pub fn new() -> Self {
        Self
    }

    pub fn dispatch(&self, event: RuntimeEvent) -> String {
        let invoke = |event| self.handle_event(event);
        invoke(event)
    }
}

impl EventHandler for RuntimeService {
    fn handle_event(&self, event: RuntimeEvent) -> String {
        match event {
            RuntimeEvent::Start(payload) => payload,
            RuntimeEvent::Stop => trace_event!(RuntimeEvent::Stop),
        }
    }
}
"#;

const RUST_FAKE_SERVICE: &str = r#"
struct RuntimeService;

impl RuntimeService {
    fn dispatch(&self) {}
}
"#;

const BASH_INSTALL: &str = r#"
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/runtime.sh
. "$SCRIPT_DIR/../lib/runtime.sh"

rk_install_main() {
  local command="${1:-install}"
  case "$command" in
    install) rk_runtime_dispatch "install" ;;
    doctor) rk_runtime_dispatch "doctor" ;;
    *) rk_missing_command "$command" ;;
  esac
}

rk_install_main "$@"
"#;

const BASH_RUNTIME: &str = r#"
rk_runtime_dispatch() {
  local mode="$1"
  rk_prepare_home "$mode"
  rk_download_artifact "$mode"
}

rk_prepare_home() {
  mkdir -p "${RK_HOME:-$HOME/.relay-knowledge}/$1"
}

rk_download_artifact() {
  printf 'download:%s\n' "$1"
}

rk_missing_command() {
  printf 'missing:%s\n' "$1" >&2
  return 64
}
"#;

const BASH_FAKE_RUNTIME: &str = r#"
rk_runtime_dispatch() {
  echo fake
}
"#;

const CSHARP_BUFFER_POOL: &str = r#"
using System;
using System.Buffers;

namespace Syntax.Runtime;

public interface IBufferSink<T>
{
    void Write(T item);
}

public sealed class BufferPoolSink : IBufferSink<byte[]>
{
    public void Write(byte[] item)
    {
        ArrayPool<byte>.Shared.Return(item);
    }

    public byte[] RentBuffer(int size)
    {
        return ArrayPool<byte>.Shared.Rent(size);
    }
}
"#;

const CSHARP_RUNTIME_SERVICE: &str = r#"
using System;
using Syntax.Runtime;

namespace Syntax.Runtime;

public sealed class RuntimeService
{
    private readonly BufferPoolSink sink = new();

    public void Dispatch(int size)
    {
        var buffer = sink.RentBuffer(size);
        Func<byte[], byte[]> returnBuffer = rented => rented;
        sink.Write(returnBuffer(buffer));
    }
}
"#;

const CSHARP_FAKE_SERVICE: &str = r#"
namespace Syntax.Runtime.Tests;

public sealed class RuntimeService
{
    public void Dispatch() {}
}
"#;

const KOTLIN_CLIENT: &str = r#"
package example

import kotlin.time.Duration

typealias RequestHandler = (String) -> String

object ClientRegistry {
    fun defaultHandler(): RequestHandler = { value -> value.trim() }
}

class SyntaxClient(private val handler: RequestHandler = ClientRegistry.defaultHandler()) {
    fun newCall(request: String): String {
        return handler(request)
    }

    companion object {
        fun withTimeout(timeout: Duration): SyntaxClient {
            return SyntaxClient { value -> "$timeout:$value" }
        }
    }
}
"#;

const KOTLIN_PIPELINE: &str = r#"
package example

fun runClientPipeline(values: List<String>): List<String> {
    val client = SyntaxClient()
    return values.map { value -> client.newCall(value) }
}
"#;

const KOTLIN_FAKE_CLIENT: &str = r#"
package example

class SyntaxClient {
    fun newCall(): String = "fake"
}
"#;

const PHP_KERNEL: &str = r#"<?php
namespace App;

use App\Contracts\Bootable;
use App\Providers\CacheProvider;

final class Kernel implements Bootable
{
    public function __construct(private CacheProvider $provider) {}

    public function boot(): void
    {
        $this->provider->register();
    }
}
"#;

const PHP_BOOTABLE: &str = r#"<?php
namespace App\Contracts;

interface Bootable
{
    public function boot(): void;
}
"#;

const PHP_CACHE_PROVIDER: &str = r#"<?php
namespace App\Providers;

trait LogsBoot
{
    public function logBoot(string $name): string
    {
        $normalizer = fn(string $value): string => trim($value);
        return $normalizer($name);
    }
}

final class CacheProvider
{
    use LogsBoot;

    public function register(): void
    {
        $this->logBoot('cache');
    }
}
"#;

const PHP_FAKE_KERNEL: &str = r#"<?php
namespace Tests;

final class Kernel
{
    public function boot(): void {}
}
"#;

const RUBY_CONTROLLER: &str = r#"
require_relative "extensions"

module App
  class Controller
    include Extensions

    def self.build
      new(Runtime.new)
    end

    def initialize(runtime)
      @runtime = runtime
    end

    def dispatch(event)
      normalize_event(@runtime.handle(event))
    end
  end
end
"#;

const RUBY_EXTENSIONS: &str = r#"
module App
  module Extensions
    def normalize_event(event)
      event.to_s.strip
    end
  end
end
"#;

const RUBY_RUNTIME: &str = r#"
module App
  class Runtime
    def handle(event)
      normalizer = ->(payload) { payload.to_s.strip }
      normalizer.call(event)
    end
  end
end
"#;

const RUBY_FAKE_CONTROLLER: &str = r#"
class Controller
  def dispatch(event)
    event
  end
end
"#;

const SCALA_PIPELINE: &str = r#"
package example

import example.Runtime.Event

trait Stage:
  def run(event: Event): Event

object Pipeline:
  inline def identityStage: Stage = new Stage:
    def run(event: Event): Event = event

  def execute(events: List[Event]): List[Event] =
    val invoke: Event => Event = event => identityStage.run(event)
    events.map(invoke)
"#;

const SCALA_RUNTIME: &str = r#"
package example

object Runtime:
  case class Event(payload: String)

class RuntimeService(stage: Stage):
  def dispatch(event: Runtime.Event): Runtime.Event =
    stage.run(event)
"#;

const SCALA_FAKE_PIPELINE: &str = r#"
package example

object Pipeline:
  def execute(): Unit = ()
"#;

const SWIFT_SESSION_CLIENT: &str = r#"
import Foundation

protocol SessionTransport {
    func send(_ request: URLRequest) async throws -> Data
}

final class SessionClient {
    private let transport: SessionTransport

    init(transport: SessionTransport) {
        self.transport = transport
    }

    func request(url: URL) async throws -> Data {
        let request = URLRequest(url: url)
        return try await transport.send(request)
    }
}
"#;

const SWIFT_REQUEST_PIPELINE: &str = r#"
import Foundation

struct RequestPipeline {
    let client: SessionClient

    func dispatch(urls: [URL]) async throws -> [Data] {
        let request = { (url: URL) async throws -> Data in
            try await client.request(url: url)
        }
        var output: [Data] = []
        for url in urls {
            output.append(try await request(url))
        }
        return output
    }
}
"#;

const SWIFT_FAKE_SESSION_CLIENT: &str = r#"
import Foundation

final class SessionClient {
    func request() {}
}
"#;

const NONSTANDARD_PYTHON_SESSION_CLIENT: &str = r#"
class ExternalPythonSessionClient:
    def open_external_session(self, payload):
        return f"python-session:{payload}"
"#;

const NONSTANDARD_TYPESCRIPT_SESSION_CLIENT: &str = r#"
export class ExternalTypeScriptSessionClient {
  openExternalSession(payload: string): string {
    return `typescript-session:${payload}`;
  }
}
"#;

const NONSTANDARD_GO_SESSION_CLIENT: &str = r#"
package session

import "context"

type ExternalGoSessionClient interface {
    OpenExternalSession(ctx context.Context, payload string) error
}
"#;

const NONSTANDARD_JAVA_SESSION_CLIENT: &str = r#"
package example;

public class ExternalJavaSessionClient {
    public String openExternalSession(String payload) {
        return "java-session:" + payload;
    }
}
"#;

const NONSTANDARD_CPP_SESSION_CLIENT_HPP: &str = r#"#pragma once

namespace nonstandard {

class ExternalCppSessionClient {
public:
    void openExternalSession();
};

void external_session_client();

}  // namespace nonstandard
"#;

const NONSTANDARD_CPP_SESSION_CLIENT_CPP: &str = r#"#include <external_session_client.hpp>

namespace nonstandard {

void ExternalCppSessionClient::openExternalSession() {
    external_session_client();
}

void external_session_client() {}

}  // namespace nonstandard
"#;

const NONSTANDARD_SWIFT_SESSION_CLIENT: &str = r#"
import Foundation

final class ExternalSwiftSessionClient {
    func openExternalSession(payload: String) -> String {
        "swift-session:\(payload)"
    }
}
"#;

const NONSTANDARD_APPLICATION_TS: &str = r#"
import { ExternalTypeScriptSessionClient } from "ts_sdk/sessionClient";

export function runExternalSessionWorkflow(payload: string): string {
  const client = new ExternalTypeScriptSessionClient();
  return client.openExternalSession(payload);
}
"#;

const NONSTANDARD_CARGO_TOML: &str = r#"
[package]
name = "nonstandard-layout-fixture"
version = "0.1.0"

[dependencies]
serde = "1.0"
"#;

const NONSTANDARD_CARGO_LOCK: &str = r#"
version = 3

[[package]]
name = "tokio"
version = "1.36.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

const NONSTANDARD_PACKAGE_JSON: &str = r#"
{
  "name": "nonstandard-layout-fixture",
  "version": "0.1.0",
  "dependencies": {
    "react": "^18.2.0"
  }
}
"#;

const NONSTANDARD_PACKAGE_LOCK_JSON: &str = r#"
{
  "lockfileVersion": 3,
  "packages": {
    "node_modules/vite": {
      "version": "5.1.0"
    }
  }
}
"#;

const NONSTANDARD_GO_MOD: &str = r#"
module example.com/nonstandard

go 1.22

require google.golang.org/grpc v1.62.0
"#;

const NONSTANDARD_PYPROJECT_TOML: &str = r#"
[project]
name = "nonstandard-layout-fixture"
dependencies = [
  "requests>=2.31",
]
"#;

const NONSTANDARD_POM_XML: &str = r#"
<project>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-dependencies</artifactId>
        <version>3.2.0</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#;

const NONSTANDARD_BUILD_GRADLE_KTS: &str = r#"
plugins {
    java
}

dependencies {
    implementation("org.slf4j:slf4j-api:2.0.9")
}
"#;

const NONSTANDARD_CONANFILE_TXT: &str = r#"
[requires]
zlib/1.2.13
"#;

const NONSTANDARD_CONANFILE_PY: &str = r#"
from conan import ConanFile

class NonstandardConan(ConanFile):
    def requirements(self):
        self.requires("openssl/3.2.1")
"#;

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
        guardrail: is_guardrail_case(case),
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
        guardrail: is_guardrail_case(case),
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
