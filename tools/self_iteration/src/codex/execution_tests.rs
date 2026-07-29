use super::*;

#[test]
fn dry_run_returns_command_without_invoking_codex() {
    let config = Config::parse(vec![
        "once".to_owned(),
        "--workspace".to_owned(),
        "/tmp/relay-knowledge".to_owned(),
        "--dry-run-codex".to_owned(),
    ])
    .expect("config should parse");

    let result = run_codex(&config, "unused prompt");

    assert!(result.succeeded());
    assert!(result.command.iter().any(|item| item == "codex"));
    assert_eq!(result.stdout, "dry-run: codex was not invoked\n");
    assert!(result.stderr.is_empty());
}
