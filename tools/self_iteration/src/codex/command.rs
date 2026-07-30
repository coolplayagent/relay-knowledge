use crate::config::{Config, DEFAULT_CODEX_MODEL};

pub(super) fn build_codex_command(config: &Config) -> Vec<String> {
    let codex = config
        .codex_path
        .clone()
        .unwrap_or_else(|| "codex".to_owned());
    let mut command = vec![codex];
    if config.yolo {
        command.extend(["-a".to_owned(), "never".to_owned()]);
    }
    command.extend([
        "exec".to_owned(),
        "-C".to_owned(),
        config.workspace.display().to_string(),
    ]);
    if config.yolo {
        command.extend([
            "--dangerously-bypass-approvals-and-sandbox".to_owned(),
            "-s".to_owned(),
            "danger-full-access".to_owned(),
        ]);
    }
    if let Some(profile) = &config.codex_profile {
        command.extend(["-p".to_owned(), profile.clone()]);
    }
    let model = config.model.as_deref().unwrap_or(DEFAULT_CODEX_MODEL);
    command.extend(["-m".to_owned(), model.to_owned()]);
    command.extend([
        "-c".to_owned(),
        format!(
            "model_reasoning_effort=\"{}\"",
            config.codex_reasoning_effort
        ),
    ]);
    command.push("-".to_owned());
    command
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod command_tests;
