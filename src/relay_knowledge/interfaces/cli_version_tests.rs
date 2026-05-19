use serde_json::Value;

use super::*;

#[test]
fn parses_version_check_command() {
    let command =
        CliCommand::parse(["version", "check", "--format", "json"]).expect("command should parse");

    assert_eq!(command.action, CliAction::VersionCheck);
    assert_eq!(command.format, OutputFormat::Json);
}

#[test]
fn version_check_help_is_machine_readable() {
    let help = cli_spec::render_help(
        &["version".to_owned(), "check".to_owned()],
        OutputFormat::Json,
    )
    .expect("help should render");
    let value: Value = serde_json::from_str(help.trim()).expect("help should be JSON");

    assert_eq!(value["operation"], "version.check");
    assert_eq!(value["effect"], "read-only");
    assert_eq!(
        value["output_formats"],
        serde_json::json!(["text", "json", "markdown"])
    );
}

#[tokio::test]
async fn run_process_does_not_append_notice_when_not_interactive() {
    let output = run_process(["--version"], false)
        .await
        .expect("version should run");

    assert!(output.stdout.contains(env!("CARGO_PKG_VERSION")));
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn process_update_notice_skips_noninteractive_commands() {
    let notice = process_update_notice(["status"], false).await;

    assert!(notice.is_none());
}
