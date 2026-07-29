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
