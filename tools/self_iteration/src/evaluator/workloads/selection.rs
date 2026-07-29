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

    fn runs_agent_workflows(&self, profile: &str) -> bool {
        self.contains(EvaluationCategory::AgentWorkflows)
            || (!self.focused() && profile_runs_slow_suites(profile))
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
        if !self.runs_agent_workflows(profile) {
            skipped.push("agent_workflows");
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
                "index_performance_many_files".to_owned(),
                "c_syntax_fixture".to_owned(),
                "cpp_syntax_fixture".to_owned(),
                "cross_language_syntax_fixture".to_owned(),
                "typescript_syntax_fixture".to_owned(),
                "nonstandard_layout_fixture".to_owned(),
                "software_global_fixture".to_owned(),
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
