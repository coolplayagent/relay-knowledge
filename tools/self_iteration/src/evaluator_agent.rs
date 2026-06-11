fn evaluate_agent_workflows(
    runtime: &EvalRuntime,
    run_home: &Path,
    cases_config: &Value,
    repositories: &BTreeMap<String, Value>,
    profile: &str,
    categories: Option<&CategorySet>,
) -> Result<Vec<RepoReport>, String> {
    let selected_cases = select_agent_workflow_cases_for_profile(
        profile,
        categories,
        array_field(cases_config, "agent_workflow_cases").to_vec(),
    );
    let grouped = objects_by_repository(&selected_cases);
    let mut reports = Vec::new();
    for (repo_name, workflow_cases) in grouped {
        let Some(repo_config) = repositories.get(&repo_name) else {
            reports.push(agent_missing_repository_report(repo_name, workflow_cases));
            continue;
        };
        reports.push(evaluate_agent_workflow_repository(
            runtime,
            run_home,
            &repo_name,
            repo_config,
            workflow_cases,
        )?);
    }
    Ok(reports)
}

fn select_agent_workflow_cases_for_profile(
    profile: &str,
    categories: Option<&CategorySet>,
    cases: Vec<Value>,
) -> Vec<Value> {
    if categories.is_some_and(|items| !items.contains(EvaluationCategory::AgentWorkflows)) {
        return Vec::new();
    }
    cases
        .into_iter()
        .filter(|case| agent_workflow_case_in_profile(profile, case))
        .collect()
}

fn agent_workflow_case_in_profile(profile: &str, case: &Value) -> bool {
    match string_field(case, "profile") {
        Some("exhaustive") => profile == "exhaustive",
        Some("full") => matches!(profile, "full" | "exhaustive"),
        _ => profile != "smoke",
    }
}

fn agent_missing_repository_report(repo_name: String, workflow_cases: Vec<Value>) -> RepoReport {
    let failed = CommandResult {
        name: format!("{repo_name}_agent_repository_config"),
        command: vec!["validate".to_owned(), "agent-workflow-repository".to_owned()],
        exit_code: 1,
        duration_ms: 0,
        stdout: String::new(),
        stderr: format!("agent workflow repository is not configured: {repo_name}"),
    };
    let cases = workflow_cases
        .iter()
        .map(|case| failed_agent_workflow_case(case, &repo_name, &failed))
        .collect();
    repo_report(&repo_name, "all".to_owned(), vec![failed], cases, Vec::new(), Value::Null)
}

fn evaluate_agent_workflow_repository(
    runtime: &EvalRuntime,
    run_home: &Path,
    repo_name: &str,
    repo_config: &Value,
    workflow_cases: Vec<Value>,
) -> Result<RepoReport, String> {
    let alias = string_or(repo_config, "alias", repo_name);
    let ref_selector = string_or(repo_config, "ref", "HEAD");
    let mut commands = Vec::new();
    let mut cases = Vec::new();
    let mut gates = Vec::new();
    let mut metrics = Vec::new();
    let (path, setup_commands) =
        prepare_repository_path(runtime, run_home, repo_name, repo_config)?;
    let setup_passed = setup_commands.iter().all(CommandResult::passed);
    commands.extend(setup_commands);
    if !setup_passed {
        return Ok(repo_report(
            repo_name,
            "all".to_owned(),
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }

    let register = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_agent_register"),
            register_command(&runtime.binary, &path, Some(alias)),
            &runtime.workspace,
            Some(runtime.env.clone()),
            runtime.timeout,
        ),
    );
    commands.push(register.clone());
    if !register.passed() {
        return Ok(repo_report(
            repo_name,
            "all".to_owned(),
            commands,
            cases,
            metrics,
            Value::Null,
        ));
    }

    let index = run_writer_limited(
        runtime,
        CommandSpec::new(
            format!("{repo_name}_agent_index"),
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
        name: format!("{repo_name}_agent_register_index_ms"),
        value: (register.duration_ms + index.duration_ms) as f64,
        budget: budget(repo_config, "agent_register_index_budget_ms"),
        lower_is_better: true,
        key: true,
    });
    commands.push(index.clone());
    if !index.passed() {
        return Ok(repo_report(
            repo_name, "all".to_owned(), commands, cases, metrics, index_json,
        ));
    }

    for workflow_case in workflow_cases {
        let (workflow_commands, observation, workflow_metrics) =
            run_agent_workflow_case(runtime, repo_name, alias, ref_selector, &workflow_case);
        if let Some(gate) =
            guardrail_gate_from_case(&observation, workflow_metrics.total_duration_ms)
        {
            gates.push(gate);
        }
        commands.extend(workflow_commands);
        metrics.extend(workflow_metrics.into_metric_observations(&workflow_case));
        cases.push(observation);
    }

    let mut report = repo_report(repo_name, "all".to_owned(), commands, cases, metrics, index_json);
    report.gates = gates;
    Ok(report)
}

#[derive(Debug, Clone, Default)]
struct AgentWorkflowMetrics {
    tool_calls: usize,
    source_reads: usize,
    output_chars: usize,
    context_chars: usize,
    evidence_hits: usize,
    text_fallback_hits: usize,
    total_context_hits: usize,
    total_duration_ms: u64,
}

impl AgentWorkflowMetrics {
    fn into_metric_observations(self, case: &Value) -> Vec<MetricObservation> {
        let case_id = string_or(case, "id", "agent_workflow");
        vec![
            lower_metric(case_id, "tool_calls", self.tool_calls as f64, case, "max_tool_calls"),
            lower_metric(
                case_id,
                "source_reads",
                self.source_reads as f64,
                case,
                "max_source_reads",
            ),
            lower_metric(
                case_id,
                "output_chars",
                self.output_chars as f64,
                case,
                "max_output_chars",
            ),
            lower_metric(
                case_id,
                "context_chars",
                self.context_chars as f64,
                case,
                "max_context_chars",
            ),
            higher_metric(
                case_id,
                "evidence_hits",
                self.evidence_hits as f64,
                case,
                "min_evidence_hits",
            ),
            lower_metric(
                case_id,
                "text_fallback_ratio",
                fallback_ratio(self.text_fallback_hits, self.total_context_hits),
                case,
                "max_text_fallback_ratio",
            ),
            lower_metric(
                case_id,
                "total_duration_ms",
                self.total_duration_ms as f64,
                case,
                "max_total_duration_ms",
            ),
        ]
    }
}

fn lower_metric(
    case_id: &str,
    suffix: &str,
    value: f64,
    case: &Value,
    budget_field: &str,
) -> MetricObservation {
    MetricObservation {
        name: format!("agent_{case_id}_{suffix}"),
        value,
        budget: budget(case, budget_field),
        lower_is_better: true,
        key: true,
    }
}

fn higher_metric(
    case_id: &str,
    suffix: &str,
    value: f64,
    case: &Value,
    budget_field: &str,
) -> MetricObservation {
    MetricObservation {
        name: format!("agent_{case_id}_{suffix}"),
        value,
        budget: budget(case, budget_field),
        lower_is_better: false,
        key: true,
    }
}

fn fallback_ratio(text_fallback_hits: usize, total_context_hits: usize) -> f64 {
    if total_context_hits == 0 {
        0.0
    } else {
        text_fallback_hits as f64 / total_context_hits as f64
    }
}

fn run_agent_workflow_case(
    runtime: &EvalRuntime,
    repo_name: &str,
    alias: &str,
    ref_selector: &str,
    workflow_case: &Value,
) -> (Vec<CommandResult>, CaseObservation, AgentWorkflowMetrics) {
    let mut commands = Vec::new();
    let mut failures = Vec::new();
    let mut metrics = AgentWorkflowMetrics::default();
    let mut source_paths = BTreeSet::new();
    let mut best_rank = None;
    let mut false_positive_count = 0usize;
    let steps = array_field(workflow_case, "steps");
    for (index, step) in steps.iter().enumerate() {
        let step_id = string_or(step, "id", "step");
        let command = run_limited(
            &runtime.limiter,
            CommandSpec::new(
                format!(
                    "{}_agent_{}_{}",
                    repo_name,
                    string_or(workflow_case, "id", "case"),
                    step_id
                ),
                agent_query_command(&runtime.binary, alias, ref_selector, step),
                &runtime.workspace,
                Some(runtime.env.clone()),
                runtime.timeout,
            ),
        );
        metrics.tool_calls += 1;
        metrics.output_chars += command.stdout.chars().count() + command.stderr.chars().count();
        metrics.total_duration_ms += command.duration_ms;
        if !command.passed() {
            failures.push(format!("step[{index}] {step_id} command failed: {}", command.gate_message()));
            commands.push(command);
            continue;
        }
        let Some(payload) = parse_json_output_value(&command.stdout) else {
            failures.push(format!("step[{index}] {step_id} emitted invalid JSON"));
            commands.push(command);
            continue;
        };
        let hits = score_array_field(&payload, "results");
        let expected = score_array_field(step, "expected");
        let forbidden = score_array_field(step, "forbidden");
        let assessment = assess_ranked_hits(step, hits, expected, forbidden);
        if best_rank.is_none_or(|rank| assessment.rank.is_some_and(|step_rank| step_rank < rank)) {
            best_rank = assessment.rank;
        }
        false_positive_count += assessment.false_positive_count;
        if !assessment.failures.is_empty() {
            failures.push(format!(
                "step[{index}] {step_id} {}",
                assessment.failures.join("; ")
            ));
        }
        metrics.evidence_hits += matched_expected_count(hits, expected);
        for hit in hits.iter().take(number_or(step, "context_hit_limit", 3) as usize) {
            metrics.total_context_hits += 1;
            metrics.context_chars += hit_context_chars(hit);
            if let Some(path) = hit.get("path").and_then(Value::as_str) {
                source_paths.insert(path.to_owned());
            }
            if hit_has_retrieval_layer(hit, "text_fallback") {
                metrics.text_fallback_hits += 1;
            }
        }
        commands.push(command);
    }
    metrics.source_reads = source_paths.len();
    failures.extend(agent_budget_failures(workflow_case, &metrics));
    let passed = failures.is_empty();
    let observation = CaseObservation {
        case_id: string_or(workflow_case, "id", "agent_workflow").to_owned(),
        repository: repo_name.to_owned(),
        passed,
        guardrail: is_guardrail_case(workflow_case),
        rank: best_rank,
        max_rank: number_or(workflow_case, "max_rank", 1) as usize,
        false_positive_count,
        message: if passed {
            format!(
                "steps={} tool_calls={} source_reads={} evidence_hits={} output_chars={} context_chars={} fallback_ratio={:.3} duration_ms={}",
                steps.len(),
                metrics.tool_calls,
                metrics.source_reads,
                metrics.evidence_hits,
                metrics.output_chars,
                metrics.context_chars,
                fallback_ratio(metrics.text_fallback_hits, metrics.total_context_hits),
                metrics.total_duration_ms
            )
        } else {
            failures.join("; ")
        },
        objective: string_or(workflow_case, "objective", "competitive_capability").to_owned(),
        score_override: Some(if passed { 1.0 } else { 0.0 }),
    };
    (commands, observation, metrics)
}

fn failed_agent_workflow_case(
    case: &Value,
    repository: &str,
    result: &CommandResult,
) -> CaseObservation {
    CaseObservation {
        case_id: string_or(case, "id", "agent_workflow").to_owned(),
        repository: repository.to_owned(),
        passed: false,
        guardrail: is_guardrail_case(case),
        rank: None,
        max_rank: number_or(case, "max_rank", 1) as usize,
        false_positive_count: 0,
        message: result.gate_message(),
        objective: string_or(case, "objective", "competitive_capability").to_owned(),
        score_override: Some(0.0),
    }
}

fn matched_expected_count(hits: &[Value], expected: &[Value]) -> usize {
    expected
        .iter()
        .filter(|pattern| hits.iter().any(|hit| hit_matches_any(hit, std::slice::from_ref(pattern))))
        .count()
}

fn hit_context_chars(hit: &Value) -> usize {
    ["path", "language_id", "excerpt", "degraded_reason", "edge_kind"]
        .iter()
        .filter_map(|field| hit.get(*field).and_then(Value::as_str))
        .map(str::chars)
        .map(Iterator::count)
        .sum()
}

fn hit_has_retrieval_layer(hit: &Value, expected_layer: &str) -> bool {
    hit.get("retrieval_layers")
        .and_then(Value::as_array)
        .map(|layers| layers.iter().any(|layer| layer.as_str() == Some(expected_layer)))
        .unwrap_or(false)
}

fn agent_budget_failures(case: &Value, metrics: &AgentWorkflowMetrics) -> Vec<String> {
    let mut failures = Vec::new();
    push_max_budget_failure(&mut failures, case, "max_tool_calls", metrics.tool_calls as f64);
    push_max_budget_failure(
        &mut failures,
        case,
        "max_source_reads",
        metrics.source_reads as f64,
    );
    push_max_budget_failure(
        &mut failures,
        case,
        "max_output_chars",
        metrics.output_chars as f64,
    );
    push_max_budget_failure(
        &mut failures,
        case,
        "max_context_chars",
        metrics.context_chars as f64,
    );
    push_max_budget_failure(
        &mut failures,
        case,
        "max_text_fallback_ratio",
        fallback_ratio(metrics.text_fallback_hits, metrics.total_context_hits),
    );
    push_max_budget_failure(
        &mut failures,
        case,
        "max_total_duration_ms",
        metrics.total_duration_ms as f64,
    );
    if let Some(minimum) = budget(case, "min_evidence_hits") {
        let actual = metrics.evidence_hits as f64;
        if actual < minimum {
            failures.push(format!("min_evidence_hits actual={actual:.3} budget={minimum:.3}"));
        }
    }
    failures
}

fn push_max_budget_failure(failures: &mut Vec<String>, case: &Value, field: &str, actual: f64) {
    if let Some(maximum) = budget(case, field) {
        if actual > maximum {
            failures.push(format!("{field} actual={actual:.3} budget={maximum:.3}"));
        }
    }
}

fn agent_query_command(binary: &Path, alias: &str, ref_selector: &str, step: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "repo".to_owned(),
        "query".to_owned(),
        alias.to_owned(),
        "--query".to_owned(),
        string_or(step, "query", "").to_owned(),
        "--kind".to_owned(),
        string_or(step, "kind", "hybrid").to_owned(),
        "--ref".to_owned(),
        string_or(step, "ref", ref_selector).to_owned(),
        "--freshness".to_owned(),
        string_or(step, "freshness", "wait-until-fresh").to_owned(),
        "--limit".to_owned(),
        number_or(step, "limit", 10).to_string(),
    ];
    for path in string_vec(step, "path_filters") {
        command.extend(["--path".to_owned(), path]);
    }
    for language in string_vec(step, "language_filters") {
        command.extend(["--language".to_owned(), language]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

const AGENT_WORKFLOW_CARGO_TOML: &str = r#"[package]
name = "agent-workflow-fixture"
version = "0.1.0"
edition = "2021"
"#;

const AGENT_WORKFLOW_CORE_CONTEXT_RS: &str = r#"pub struct AgentContextPackBuilder {
    max_context_chars: usize,
    evidence_floor: usize,
}

impl AgentContextPackBuilder {
    pub fn new(max_context_chars: usize, evidence_floor: usize) -> Self {
        Self {
            max_context_chars,
            evidence_floor,
        }
    }

    pub fn build_context_packet(&self, request: &AgentWorkflowRequest) -> AgentContextPacket {
        let summary = format!(
            "{}:{}:{}",
            request.repository_alias, self.max_context_chars, self.evidence_floor
        );
        AgentContextPacket {
            summary,
            freshness_mode: request.freshness_mode.clone(),
        }
    }
}

pub struct AgentWorkflowRequest {
    pub repository_alias: String,
    pub freshness_mode: String,
}

pub struct AgentContextPacket {
    pub summary: String,
    pub freshness_mode: String,
}
"#;

const AGENT_WORKFLOW_CORE_ORCHESTRATOR_RS: &str = r#"use crate::context::{AgentContextPackBuilder, AgentWorkflowRequest};

pub struct AgentWorkflowOrchestrator {
    context_builder: AgentContextPackBuilder,
}

impl AgentWorkflowOrchestrator {
    pub fn new(context_builder: AgentContextPackBuilder) -> Self {
        Self { context_builder }
    }

    pub fn analyze_issue_entrypoint(&self, request: &AgentWorkflowRequest) -> String {
        let packet = self.context_builder.build_context_packet(request);
        format!("{}:{}", packet.summary, packet.freshness_mode)
    }
}
"#;

const AGENT_WORKFLOW_CORE_LIB_RS: &str = r#"pub mod context;
pub mod orchestrator;
"#;

const AGENT_WORKFLOW_WEB_CONTEXT_TS: &str = r#"export type AgentEvidenceCard = {
  path: string;
  excerpt: string;
  retrievalLayer: string;
};

export function buildContextPacket(cards: AgentEvidenceCard[], maxContextChars: number): string {
  return cards
    .slice(0, 4)
    .map((card) => `${card.path}:${card.retrievalLayer}:${card.excerpt}`)
    .join("\n")
    .slice(0, maxContextChars);
}
"#;

const AGENT_WORKFLOW_WEB_ENTRY_TS: &str = r#"import { buildContextPacket, AgentEvidenceCard } from "./contextPacket";

export function renderAgentWorkflowAnswer(cards: AgentEvidenceCard[]): string {
  return buildContextPacket(cards, 4096);
}
"#;

const AGENT_WORKFLOW_OPS_POLICY_PY: &str = r#"AGENT_POLICY_BUDGET = {
    "max_tool_calls": 6,
    "max_source_reads": 8,
    "max_context_chars": 9000,
    "freshness": "wait-until-fresh",
}


def load_agent_policy(environment: str) -> dict[str, object]:
    policy = dict(AGENT_POLICY_BUDGET)
    policy["environment"] = environment
    return policy
"#;

const AGENT_WORKFLOW_CONFIG_YAML: &str = r#"agent_workflow:
  max_tool_calls: 6
  max_source_reads: 8
  max_output_chars: 64000
  freshness_state: wait-until-fresh
  fallback_policy: bounded-search
"#;

const AGENT_WORKFLOW_DOC_MD: &str = r#"# Agent Workflow Evaluation Fixture

The coding-agent workflow combines definition lookup, cross-language context packet construction,
configuration tracing, and freshness policy verification. The expected answer must cite structured
evidence before bounded text fallback and keep the packed context under the configured budget.

Freshness scenarios use wait-until-fresh for normal issue analysis and allow-stale only when a
caller explicitly accepts stale graph evidence while diagnostics report the freshness state.
"#;
