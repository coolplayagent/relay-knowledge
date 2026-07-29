use super::{http_judge_content, judge_cli_command, judge_http_command, normalize_judge_chat_url};
use crate::evaluator::judge::settings::{JudgeBackend, JudgeSettings};

fn http_settings() -> JudgeSettings {
    JudgeSettings {
        enabled: true,
        backend: JudgeBackend::Http,
        missing: Vec::new(),
        configuration_error: None,
        command: String::new(),
        http_base_url: "http://localhost:11434/v1".to_owned(),
        http_api_key: "token".to_owned(),
        http_model: "judge-model".to_owned(),
        timeout_seconds: 30,
    }
}

#[test]
fn cli_command_expands_paths_and_uses_stdin_without_a_prompt_placeholder() {
    let workspace = std::path::Path::new("/workspace");
    let prompt_file = workspace.join("judge-prompt.txt");

    let (command, stdin) = judge_cli_command(
        "judge --workspace {workspace} --file {prompt_file}",
        workspace,
        &prompt_file,
        "prompt",
    )
    .expect("CLI command should parse");

    assert_eq!(
        command,
        vec![
            "judge",
            "--workspace",
            "/workspace",
            "--file",
            "/workspace/judge-prompt.txt"
        ]
    );
    assert_eq!(stdin, None);

    let (_, stdin) =
        judge_cli_command("judge", workspace, &prompt_file, "prompt").expect("command");
    assert_eq!(stdin.as_deref(), Some("prompt"));
}

#[test]
fn http_command_keeps_secrets_in_environment_and_extracts_response_content() {
    let settings = http_settings();

    let (command, body) = judge_http_command(&settings, "judge prompt").expect("http command");

    assert!(!command.join(" ").contains("token"));
    assert!(body.contains("judge-model"));
    assert!(body.contains("judge prompt"));
    assert_eq!(
        normalize_judge_chat_url(&settings.http_base_url),
        "http://localhost:11434/v1/chat/completions"
    );
    assert_eq!(
        http_judge_content(r#"{"choices":[{"message":{"content":"{\"passed\":true}"}}]}"#)
            .as_deref(),
        Some(r#"{"passed":true}"#)
    );
}
