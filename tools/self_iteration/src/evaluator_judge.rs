#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JudgeBackend {
    Cli,
    Http,
}

impl JudgeBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Clone)]
struct JudgeSettings {
    enabled: bool,
    backend: JudgeBackend,
    missing: Vec<String>,
    configuration_error: Option<String>,
    command: String,
    http_base_url: String,
    http_api_key: String,
    http_model: String,
    timeout_seconds: u64,
}

fn judge_settings(env: &BTreeMap<String, String>) -> JudgeSettings {
    let backend_value = env
        .get("RELAY_KNOWLEDGE_JUDGE_BACKEND")
        .map(|value| normalize_backend(value))
        .filter(|value| !value.is_empty());
    let timeout_seconds = env
        .get("RELAY_KNOWLEDGE_JUDGE_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(120)
        .max(1);
    if backend_value
        .as_deref()
        .is_some_and(|backend| ["none", "off", "disabled", "skip", "false"].contains(&backend))
    {
        return JudgeSettings {
            enabled: false,
            backend: JudgeBackend::Cli,
            missing: Vec::new(),
            configuration_error: None,
            command: String::new(),
            http_base_url: String::new(),
            http_api_key: String::new(),
            http_model: String::new(),
            timeout_seconds,
        };
    }
    let http_base_url = env_string(env, "RELAY_KNOWLEDGE_JUDGE_BASE_URL");
    let http_api_key = env_string(env, "RELAY_KNOWLEDGE_JUDGE_API_KEY");
    let http_model = env_string(env, "RELAY_KNOWLEDGE_JUDGE_MODEL");
    let http_env_configured =
        !http_base_url.is_empty() || !http_api_key.is_empty() || !http_model.is_empty();
    let explicit_command = [
        "RELAY_KNOWLEDGE_JUDGE_COMMAND",
        "RELAY_KNOWLEDGE_JUDGE_AGENT_COMMAND",
        "RELAY_KNOWLEDGE_JUDGE_CLI_COMMAND",
    ]
    .iter()
    .find_map(|name| env.get(*name).filter(|value| !value.trim().is_empty()).cloned());
    let command = explicit_command.clone().unwrap_or_else(|| {
        "opencode run \"Read the attached relay-knowledge judge prompt and return only the strict JSON object it requests.\" --file {prompt_file}".to_owned()
    });
    let mut configuration_error = None;
    let backend = match backend_value.as_deref() {
        Some("http" | "openai" | "openai_compatible" | "api" | "llm") => JudgeBackend::Http,
        Some("agent" | "coding_agent" | "cli_agent" | "opencode" | "open_code" | "cli") => {
            JudgeBackend::Cli
        }
        Some(other) => {
            configuration_error = Some(format!(
                "unsupported RELAY_KNOWLEDGE_JUDGE_BACKEND value: {other}"
            ));
            JudgeBackend::Cli
        }
        None if explicit_command.is_some() => JudgeBackend::Cli,
        None if http_env_configured => JudgeBackend::Http,
        None => JudgeBackend::Cli,
    };
    let missing = if backend == JudgeBackend::Http {
        [
            ("RELAY_KNOWLEDGE_JUDGE_BASE_URL", &http_base_url),
            ("RELAY_KNOWLEDGE_JUDGE_API_KEY", &http_api_key),
            ("RELAY_KNOWLEDGE_JUDGE_MODEL", &http_model),
        ]
        .into_iter()
        .filter(|(_, value)| value.is_empty())
        .map(|(name, _)| name.to_owned())
        .collect()
    } else {
        Vec::new()
    };
    JudgeSettings {
        enabled: true,
        backend,
        missing,
        configuration_error,
        command,
        http_base_url,
        http_api_key,
        http_model,
        timeout_seconds,
    }
}

fn settings_summary(settings: &JudgeSettings) -> Value {
    serde_json::json!({
        "backend": settings.backend.as_str(),
        "enabled": settings.enabled,
        "configured": settings.enabled && settings.missing.is_empty() && settings.configuration_error.is_none(),
        "missing": settings.missing,
        "configuration_error": settings.configuration_error,
        "timeout_seconds": settings.timeout_seconds,
        "cli_command_configured": settings.backend == JudgeBackend::Cli && !settings.command.is_empty(),
        "cli_command_program": shell_split(&settings.command).ok().and_then(|parts| parts.first().cloned()),
        "http_base_url_configured": !settings.http_base_url.is_empty(),
        "http_api_key_configured": !settings.http_api_key.is_empty(),
        "http_model_configured": !settings.http_model.is_empty(),
        "http_model": if settings.backend == JudgeBackend::Http { Some(settings.http_model.as_str()) } else { None },
    })
}

fn normalize_backend(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn env_string(env: &BTreeMap<String, String>, name: &str) -> String {
    env.get(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
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

fn run_judge_backend(
    input: &JudgeEvalInput<'_>,
    settings: &JudgeSettings,
    prompt_file: &Path,
    prompt: &str,
) -> Result<CommandResult, String> {
    match settings.backend {
        JudgeBackend::Cli => {
            let command =
                judge_cli_command(&settings.command, input.workspace, prompt_file, prompt)?;
            Ok(run_limited(
                input.limiter,
                CommandSpec::new(
                    "research_judge",
                    command.0,
                    input.workspace,
                    Some(input.env.clone()),
                    settings.timeout_seconds,
                )
                .with_stdin(command.1.unwrap_or_default()),
            ))
        }
        JudgeBackend::Http => {
            let command = judge_http_command(settings, prompt)?;
            let mut result = run_limited(
                input.limiter,
                CommandSpec::new(
                    "research_judge",
                    command.0,
                    input.workspace,
                    Some(input.env.clone()),
                    settings.timeout_seconds,
                )
                .with_stdin(command.1),
            );
            if result.passed() {
                result.stdout = http_judge_content(&result.stdout).unwrap_or(result.stdout);
            }
            Ok(result)
        }
    }
}

fn judge_http_command(
    settings: &JudgeSettings,
    prompt: &str,
) -> Result<(Vec<String>, String), String> {
    let url = normalize_judge_chat_url(&settings.http_base_url);
    let payload = serde_json::json!({
        "model": settings.http_model,
        "messages": [
            {"role": "system", "content": "Return only strict JSON. Do not include markdown."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0,
    });
    let body = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
    Ok((
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "curl -sS --fail-with-body --max-time \"$1\" -H \"Authorization: Bearer ${RELAY_KNOWLEDGE_JUDGE_API_KEY}\" -H \"Content-Type: application/json\" -d @- \"$2\"".to_owned(),
            "relay-knowledge-judge-http".to_owned(),
            settings.timeout_seconds.to_string(),
            url,
        ],
        body,
    ))
}

fn normalize_judge_chat_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_owned()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn http_judge_content(body: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .or_else(|| choice.get("text").and_then(Value::as_str))
        })
        .or_else(|| value.get("output_text").and_then(Value::as_str))
        .map(ToOwned::to_owned)
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
