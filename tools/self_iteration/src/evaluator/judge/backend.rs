use std::path::Path;

use serde_json::Value;

use crate::command::{CommandResult, CommandSpec};

use super::super::run_limited;
use super::{
    JudgeEvalInput,
    settings::{JudgeBackend, JudgeSettings, shell_split},
};

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

pub(super) fn run_judge_backend(
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

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
