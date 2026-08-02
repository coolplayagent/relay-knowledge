use super::*;

#[test]
fn codex_command_defaults_to_gpt56_sol_xhigh() {
    let config = Config::parse(vec![
        "once".to_owned(),
        "--workspace".to_owned(),
        "/tmp/relay-knowledge".to_owned(),
    ])
    .expect("config should parse");

    let command = build_codex_command(&config);

    assert_eq!(
        command,
        vec![
            "codex",
            "exec",
            "-C",
            "/tmp/relay-knowledge",
            "-m",
            "gpt-5.6-sol",
            "-c",
            "model_reasoning_effort=\"xhigh\"",
            "-"
        ]
    );
}

#[test]
fn codex_command_keeps_explicit_generation_overrides() {
    let config = Config::parse(vec![
        "once".to_owned(),
        "--workspace".to_owned(),
        "/tmp/relay-knowledge".to_owned(),
        "--yolo".to_owned(),
        "--codex-path".to_owned(),
        "/usr/local/bin/codex".to_owned(),
        "--model".to_owned(),
        "o3".to_owned(),
        "--codex-reasoning-effort=high".to_owned(),
        "--codex-profile".to_owned(),
        "self-iteration".to_owned(),
    ])
    .expect("config should parse");

    let command = build_codex_command(&config);

    assert_eq!(
        command,
        vec![
            "/usr/local/bin/codex",
            "-a",
            "never",
            "exec",
            "-C",
            "/tmp/relay-knowledge",
            "--dangerously-bypass-approvals-and-sandbox",
            "-s",
            "danger-full-access",
            "-p",
            "self-iteration",
            "-m",
            "o3",
            "-c",
            "model_reasoning_effort=\"high\"",
            "-"
        ]
    );
}
